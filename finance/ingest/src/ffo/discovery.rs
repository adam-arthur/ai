use std::{
    collections::HashSet, fs, path::{Path, PathBuf}
};

use anyhow::{Context, Result};
use reqwest::Url;
use scraper::{Html, Selector};
use serde::{Deserialize, Serialize};
use time::{Date, macros::format_description};

use crate::{
    common::sec_api::{SecFiling, fetch_recent_filings, fetch_sec_text}, file_utils::write_json_atomic, ingest_utils::DATA_FETCH_START_DATE
};

use super::{FfoSourceDocument, ReitFfoSources};

#[derive(Debug)]
pub(super) struct DiscoveredFiling {
    pub(super) filing: SecFiling,
    pub(super) documents: Vec<FfoSourceDocument>,
}

#[derive(Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(super) struct FilingIndexDocument {
    pub(super) url: String,
    pub(super) exhibit_type: String,
    pub(super) description: String,
}

pub(super) async fn discover_reit_ffo_sources(cik: &str) -> Result<ReitFfoSources> {
    let filings = discover_reit_ffo_filings(cik).await?;
    Ok(ReitFfoSources {
        cik: cik.trim_start_matches('0').to_owned(),
        documents: filings
            .into_iter()
            .flat_map(|filing| filing.documents)
            .collect(),
    })
}

pub(super) async fn discover_reit_ffo_filings(cik: &str) -> Result<Vec<DiscoveredFiling>> {
    discover_reit_ffo_filings_with_cache(cik, None).await
}

pub(super) async fn discover_reit_ffo_filings_to_cache(
    cik: &str,
    data_dir: &Path,
    symbol: &str,
) -> Result<Vec<DiscoveredFiling>> {
    discover_reit_ffo_filings_with_cache(cik, Some((data_dir, symbol))).await
}

async fn discover_reit_ffo_filings_with_cache(
    cik: &str,
    cache: Option<(&Path, &str)>,
) -> Result<Vec<DiscoveredFiling>> {
    let mut filings = fetch_recent_filings(cik).await?;
    filings.retain(is_within_data_fetch_period);
    filings.sort_by(|left, right| {
        filing_priority(left)
            .cmp(&filing_priority(right))
            .then_with(|| right.filing_date.cmp(&left.filing_date))
            .then_with(|| right.accession_number.cmp(&left.accession_number))
    });

    let mut discovered = Vec::new();
    let mut seen_urls = HashSet::new();
    for filing in filings {
        let filing_documents = match cache {
            Some((data_dir, symbol)) => {
                let path = filing_index_cache_path(data_dir, symbol, &filing.accession_number);
                fetch_filing_index_cached(&filing, &path).await?
            }
            None => fetch_filing_index(&filing).await?,
        };
        let mut documents = Vec::new();
        for document in filing_documents
            .into_iter()
            .filter(|document| is_likely_ffo_source(&filing, document))
        {
            if !seen_urls.insert(document.url.clone()) {
                continue;
            }
            documents.push(FfoSourceDocument {
                url: document.url,
                exhibit_type: document.exhibit_type,
                description: document.description,
                filing_date: filing.filing_date.clone(),
                accession_number: filing.accession_number.clone(),
                filing_form: filing.form.clone(),
                filing_index_url: filing.filing_index_url.clone(),
            });
        }
        discovered.push(DiscoveredFiling { filing, documents });
    }
    Ok(discovered)
}

pub(super) fn filing_index_cache_path(data_dir: &Path, symbol: &str, accession: &str) -> PathBuf {
    data_dir
        .join("sec")
        .join("filing-index")
        .join(symbol)
        .join(format!("{accession}.json"))
}

async fn fetch_filing_index_cached(
    filing: &SecFiling,
    path: &Path,
) -> Result<Vec<FilingIndexDocument>> {
    if path.is_file() {
        let bytes = fs::read(path).with_context(|| format!("failed to read {}", path.display()))?;
        return serde_json::from_slice(&bytes)
            .with_context(|| format!("failed to parse {}", path.display()));
    }

    let documents = fetch_filing_index(filing).await?;
    let parent = path
        .parent()
        .context("SEC filing-index cache path has no parent")?;
    fs::create_dir_all(parent).with_context(|| format!("failed to create {}", parent.display()))?;
    write_json_atomic(path, &documents)
        .with_context(|| format!("failed to cache SEC filing index at {}", path.display()))?;
    Ok(documents)
}

pub(super) fn is_within_data_fetch_period(filing: &SecFiling) -> bool {
    let source_date = filing.report_date.as_deref().unwrap_or(&filing.filing_date);
    Date::parse(source_date, format_description!("[year]-[month]-[day]"))
        .is_ok_and(|source_date| source_date >= DATA_FETCH_START_DATE)
}

async fn fetch_filing_index(filing: &SecFiling) -> Result<Vec<FilingIndexDocument>> {
    let html = fetch_sec_text(&filing.filing_index_url)
        .await
        .with_context(|| format!("failed to download {}", filing.filing_index_url))?;
    if html.is_empty() {
        anyhow::bail!(
            "SEC returned an empty filing index for {}",
            filing.filing_index_url
        );
    }
    Ok(parse_filing_index(&html, &filing.filing_index_url))
}

pub(super) fn parse_filing_index(html: &str, filing_index_url: &str) -> Vec<FilingIndexDocument> {
    let document = Html::parse_document(html);
    let row_selector = Selector::parse("table.tableFile tr").expect("valid row selector");
    let cell_selector = Selector::parse("td").expect("valid cell selector");
    let link_selector = Selector::parse("a[href]").expect("valid link selector");

    document
        .select(&row_selector)
        .filter_map(|row| {
            let cells = row.select(&cell_selector).collect::<Vec<_>>();
            // EDGAR's document table is: sequence, description, document, type, size.
            if cells.len() < 4 {
                return None;
            }

            let href = cells[2]
                .select(&link_selector)
                .next()?
                .value()
                .attr("href")?;
            let url = resolve_document_url(filing_index_url, href)?;
            Some(FilingIndexDocument {
                url,
                exhibit_type: normalized_text(cells[3].text()),
                description: normalized_text(cells[1].text()),
            })
        })
        .collect()
}

pub(super) fn resolve_document_url(filing_index_url: &str, href: &str) -> Option<String> {
    let base = Url::parse(filing_index_url).ok()?;
    let url = base.join(href).ok()?;

    // Interactive-data links wrap the actual archive document in a `doc` query parameter. Return
    // the underlying filing document so downstream extraction receives the issuer's HTML directly.
    if url.path().ends_with("/ixviewer/doc/action")
        && let Some((_, document_path)) = url.query_pairs().find(|(key, _)| key == "doc")
    {
        return base.join(document_path.as_ref()).ok().map(Url::into);
    }

    Some(url.into())
}

fn normalized_text<'a>(fragments: impl Iterator<Item = &'a str>) -> String {
    fragments
        .collect::<Vec<_>>()
        .join(" ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

pub(super) fn filing_priority(filing: &SecFiling) -> u8 {
    match base_form(&filing.form) {
        "8-K" if filing.items.iter().any(|item| item == "2.02") => 0,
        "10-Q" => 1,
        "10-K" => 2,
        "8-K" => 3,
        _ => 4,
    }
}

pub(super) fn is_likely_ffo_source(filing: &SecFiling, document: &FilingIndexDocument) -> bool {
    let form = base_form(&filing.form);
    let exhibit_type = document.exhibit_type.to_ascii_uppercase();
    let description = document.description.to_ascii_lowercase();
    let is_primary_report =
        document.url == filing.primary_document_url && matches!(form, "10-Q" | "10-K");
    let is_exhibit_99 = exhibit_type == "EX-99" || exhibit_type.starts_with("EX-99.");
    let has_source_description = [
        "earnings",
        "financial results",
        "supplement",
        "supplemental",
        "funds from operations",
        "ffo",
        "affo",
    ]
    .iter()
    .any(|keyword| description.contains(keyword));

    is_primary_report || is_exhibit_99 || has_source_description
}

fn base_form(form: &str) -> &str {
    form.strip_suffix("/A").unwrap_or(form)
}
