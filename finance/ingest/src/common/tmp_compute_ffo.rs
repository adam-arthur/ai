//! Temporary discovery step for locating REIT FFO/AFFO source documents.
//!
//! This module deliberately stops at document discovery. A later step can download the returned
//! documents and extract each issuer's FFO/AFFO definition and reconciliation without losing the
//! filing provenance.

use std::collections::HashSet;

use reqwest::Url;
use scraper::{Html, Selector};
use serde::Serialize;

use crate::meta_utils::YieldWatchError;

use super::sec_api::{SecFiling, fetch_recent_filings, fetch_sec_text};

/// A filing document likely to contain a reported FFO or AFFO value or reconciliation.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FfoSourceDocument {
    pub url: String,
    pub exhibit_type: String,
    pub description: String,
    pub filing_date: String,
    pub accession_number: String,
    pub filing_form: String,
    pub filing_index_url: String,
}

/// Source-document discovery results for one issuer.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReitFfoSources {
    pub cik: String,
    pub documents: Vec<FfoSourceDocument>,
}

#[derive(Debug, Eq, PartialEq)]
struct FilingIndexDocument {
    url: String,
    exhibit_type: String,
    description: String,
}

/// Locates likely FFO/AFFO source documents in a REIT's recent 8-K, 10-Q, and 10-K filings.
///
/// Item 2.02 8-K filings are fetched first, followed by 10-Q, 10-K, and other 8-K filings. The SEC
/// recent-submissions feed may omit older filings, so this function has the same recent-history
/// boundary as [`fetch_recent_filings`].
#[allow(dead_code)]
pub async fn fetch_reit_ffo_sources(cik: &str) -> Result<ReitFfoSources, YieldWatchError> {
    let mut filings = fetch_recent_filings(cik).await?;
    filings.sort_by(|left, right| {
        filing_priority(left)
            .cmp(&filing_priority(right))
            .then_with(|| right.filing_date.cmp(&left.filing_date))
            .then_with(|| right.accession_number.cmp(&left.accession_number))
    });

    let mut documents = Vec::new();
    let mut seen_urls = HashSet::new();
    for filing in filings {
        let filing_documents = fetch_filing_index(&filing).await?;
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
    }

    Ok(ReitFfoSources {
        cik: cik.trim_start_matches('0').to_owned(),
        documents,
    })
}

/// Runs source discovery for multiple REITs while preserving the caller's CIK order.
///
/// Requests are intentionally sequential because SEC.gov asks automated clients to stay below its
/// fair-access request ceiling. A failure is returned with no partial result so callers do not
/// mistake an incomplete issuer history for a successful discovery.
#[allow(dead_code)]
pub async fn fetch_reit_ffo_sources_batch(
    ciks: impl IntoIterator<Item = impl AsRef<str>>,
) -> Result<Vec<ReitFfoSources>, YieldWatchError> {
    let mut results = Vec::new();
    for cik in ciks {
        results.push(fetch_reit_ffo_sources(cik.as_ref()).await?);
    }
    Ok(results)
}

async fn fetch_filing_index(
    filing: &SecFiling,
) -> Result<Vec<FilingIndexDocument>, YieldWatchError> {
    let html = fetch_sec_text(&filing.filing_index_url).await?;
    Ok(parse_filing_index(&html, &filing.filing_index_url))
}

fn parse_filing_index(html: &str, filing_index_url: &str) -> Vec<FilingIndexDocument> {
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

fn resolve_document_url(filing_index_url: &str, href: &str) -> Option<String> {
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

fn filing_priority(filing: &SecFiling) -> u8 {
    match base_form(&filing.form) {
        "8-K" if filing.items.iter().any(|item| item == "2.02") => 0,
        "10-Q" => 1,
        "10-K" => 2,
        "8-K" => 3,
        _ => 4,
    }
}

fn is_likely_ffo_source(filing: &SecFiling, document: &FilingIndexDocument) -> bool {
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

#[cfg(test)]
mod tests {
    use super::*;

    const INDEX_HTML: &str = r#"
        <html><body>
          <table class="tableFile" summary="Document Format Files">
            <tr><th>Seq</th><th>Description</th><th>Document</th><th>Type</th><th>Size</th></tr>
            <tr>
              <td>1</td><td>FORM 8-K</td>
              <td><a href="/Archives/edgar/data/123/000012326000001/form8-k.htm">form8-k.htm</a></td>
              <td>8-K</td><td>1000</td>
            </tr>
            <tr>
              <td>2</td><td>Earnings Release&nbsp;and Supplemental</td>
              <td><a href="earnings.htm">earnings.htm</a></td><td>EX-99.1</td><td>2000</td>
            </tr>
          </table>
          <table class="tableFile" summary="Data Files">
            <tr><td>3</td><td>XBRL TAXONOMY EXTENSION SCHEMA</td>
              <td><a href="reit-20260331.xsd">reit-20260331.xsd</a></td><td>EX-101.SCH</td><td>500</td></tr>
          </table>
        </body></html>
    "#;

    fn filing(form: &str, items: &[&str], primary_document: &str) -> SecFiling {
        let archive_base = "https://www.sec.gov/Archives/edgar/data/123/000012326000001";
        SecFiling {
            accession_number: "0000123-26-000001".to_owned(),
            filing_date: "2026-05-01".to_owned(),
            report_date: Some("2026-03-31".to_owned()),
            acceptance_date_time: None,
            form: form.to_owned(),
            items: items.iter().map(|item| (*item).to_owned()).collect(),
            primary_document: primary_document.to_owned(),
            primary_document_description: None,
            filing_index_url: format!("{archive_base}/0000123-26-000001-index.html"),
            primary_document_url: format!("{archive_base}/{primary_document}"),
        }
    }

    #[test]
    fn parses_edgar_document_table() {
        let index_url = "https://www.sec.gov/Archives/edgar/data/123/000012326000001/0000123-26-000001-index.html";
        let documents = parse_filing_index(INDEX_HTML, index_url);

        assert_eq!(documents.len(), 3);
        assert_eq!(documents[1].exhibit_type, "EX-99.1");
        assert_eq!(
            documents[1].description,
            "Earnings Release and Supplemental"
        );
        assert_eq!(
            documents[1].url,
            "https://www.sec.gov/Archives/edgar/data/123/000012326000001/earnings.htm"
        );
    }

    #[test]
    fn selects_primary_periodic_reports_and_likely_earnings_exhibits() {
        let eight_k = filing("8-K", &["2.02", "9.01"], "form8-k.htm");
        let ten_q = filing("10-Q", &[], "form8-k.htm");
        let documents = parse_filing_index(INDEX_HTML, &eight_k.filing_index_url);

        assert!(!is_likely_ffo_source(&eight_k, &documents[0]));
        assert!(is_likely_ffo_source(&eight_k, &documents[1]));
        assert!(is_likely_ffo_source(&ten_q, &documents[0]));
        assert!(!is_likely_ffo_source(&eight_k, &documents[2]));
    }

    #[test]
    fn prioritizes_item_202_eight_k_before_periodic_reports() {
        assert_eq!(filing_priority(&filing("8-K", &["2.02"], "form8-k.htm")), 0);
        assert_eq!(filing_priority(&filing("10-Q", &[], "quarter.htm")), 1);
        assert_eq!(filing_priority(&filing("10-K/A", &[], "annual.htm")), 2);
        assert_eq!(filing_priority(&filing("8-K", &["7.01"], "form8-k.htm")), 3);
    }

    #[test]
    fn unwraps_inline_xbrl_viewer_links() {
        let index_url = "https://www.sec.gov/Archives/edgar/data/123/1/filing-index.html";
        let href = "/ixviewer/doc/action?doc=/Archives/edgar/data/123/1/report.htm";

        assert_eq!(
            resolve_document_url(index_url, href).as_deref(),
            Some("https://www.sec.gov/Archives/edgar/data/123/1/report.htm")
        );
    }
}
