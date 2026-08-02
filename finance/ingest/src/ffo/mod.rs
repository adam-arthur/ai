//! Pipeline for extracting canonical, period-oriented REIT FFO/AFFO actual results.
//!
//! Source documents are cached separately. Persisted issuer data contains only numeric measures
//! and parsed reconciliation components; HTML cells and other extraction internals never leak into
//! the consumer-facing model.

use std::{
    collections::{BTreeSet, HashSet}, fs, path::{Path, PathBuf}
};

use anyhow::{Context, Result};
use reqwest::Url;

mod canonical;
mod discovery;
mod extract;
mod models;
mod vision;

use models::ExtractedFfoDocument;
pub use models::{
    FfoAdjustment, FfoMeasure, FfoMeasures, FfoPeriodResult, FfoReconciliation, FfoReportingPeriod, FfoSourceDocument, ReitFfoData, ReitFfoSources
};

use crate::file_utils::write_json_atomic;
use crate::{common::sec_api::fetch_sec_document, meta_utils::get_app_data_path};

const PROCESSED_FILINGS_FILE: &str = "processed-filings.json";

/// Locates likely FFO/AFFO source documents without writing to the persistent source cache.
#[allow(dead_code)]
pub async fn fetch_reit_ffo_sources(cik: &str) -> Result<ReitFfoSources> {
    discovery::discover_reit_ffo_sources(cik).await
}

/// Runs uncached source discovery sequentially while preserving caller CIK order.
#[allow(dead_code)]
pub async fn fetch_reit_ffo_sources_batch(
    ciks: impl IntoIterator<Item = impl AsRef<str>>,
) -> Result<Vec<ReitFfoSources>> {
    let mut results = Vec::new();
    for cik in ciks {
        results.push(fetch_reit_ffo_sources(cik.as_ref()).await?);
    }
    Ok(results)
}

/// Discovers, downloads, and extracts an issuer's recent FFO/AFFO source documents.
///
/// Raw documents are archived below semantic filing directories such as
/// `sources_dir/<symbol>/2025-q4-results/<document-name>`. Processed SEC accessions, including
/// filings determined not to contain FFO material, are recorded in `processed-filings.json`.
#[allow(dead_code)]
pub async fn fetch_reit_ffo_data(
    symbol: &str,
    cik: &str,
    sources_dir: impl AsRef<Path>,
) -> Result<ReitFfoData> {
    let issuer_dir = issuer_source_dir(sources_dir.as_ref(), symbol);
    fs::create_dir_all(&issuer_dir)
        .with_context(|| format!("failed to create {}", issuer_dir.display()))?;
    let processed_path = issuer_dir.join(PROCESSED_FILINGS_FILE);
    let mut processed_accessions = read_processed_accessions(&processed_path)?;
    let filings = discovery::discover_reit_ffo_filings(cik, &processed_accessions).await?;

    let mut documents = Vec::new();
    for filing in filings {
        let accession = filing.filing.accession_number.clone();
        let mut fetched_documents = Vec::new();
        for source in filing.documents {
            let (bytes, content_type) = fetch_sec_document(&source.url)
                .await
                .with_context(|| format!("failed to download {}", source.url))?;
            if bytes.is_empty() {
                anyhow::bail!("SEC returned an empty source document for {}", source.url);
            }
            if document_mentions_ffo(&source, &bytes, content_type.as_deref()) {
                fetched_documents.push((source, bytes, content_type));
            }
        }

        if fetched_documents.is_empty() {
            mark_accession_processed(&processed_path, &mut processed_accessions, accession)?;
            continue;
        }

        let folder_name = semantic_filing_name(&filing.filing, &fetched_documents);
        let document_dir = issuer_dir.join(&folder_name);
        assert!(
            !document_dir.exists(),
            "FFO source folder collision: {} already exists while processing {}",
            document_dir.display(),
            accession
        );
        assert_unique_source_file_names(&accession, &fetched_documents);
        fs::create_dir(&document_dir)
            .with_context(|| format!("failed to create {}", document_dir.display()))?;

        let mut vision_sources = Vec::new();
        for (source, bytes, content_type) in fetched_documents {
            let local_path = document_dir.join(source_file_name(&source.url));
            assert!(
                !local_path.exists(),
                "FFO source filename collision at {} while processing {}",
                local_path.display(),
                accession
            );
            write_bytes_atomic(&local_path, &bytes)?;
            vision_sources.push((local_path, content_type, bytes));

            // HTML value extraction is intentionally paused while the screenshot-first pipeline is
            // developed. Keep the source document in the canonicalization input so SEC pulling and
            // image generation continue unchanged, but do not publish HTML-scraped values.
            documents.push(ExtractedFfoDocument {
                document: source,
                values: Vec::new(),
            });
        }

        let image_count = match vision::ensure_candidate_images(&document_dir, vision_sources).await
        {
            Ok(count) => count,
            Err(error) => {
                fs::remove_dir_all(&document_dir).with_context(|| {
                    format!(
                        "FFO vision failed and incomplete filing directory could not be removed: {}",
                        document_dir.display()
                    )
                })?;
                return Err(error)
                    .with_context(|| format!("FFO vision failed for {}", document_dir.display()));
            }
        };
        log::debug!(
            "FFO vision - cached {image_count} candidate image(s) in {}",
            document_dir.join("vision").display()
        );
        mark_accession_processed(&processed_path, &mut processed_accessions, accession)?;
    }

    Ok(canonical::canonicalize(
        symbol,
        cik.trim_start_matches('0'),
        documents,
    ))
}

/// Uses the repository's persistent source cache (`data/ffo/sources` in the default setup).
#[allow(dead_code)]
pub async fn fetch_reit_ffo_data_to_cache(symbol: &str, cik: &str) -> Result<ReitFfoData> {
    fetch_reit_ffo_data(symbol, cik, get_app_data_path().join("ffo").join("sources")).await
}

/// Sequential batch variant that honors the SEC fair-access request rate and caller issuer order.
#[allow(dead_code)]
pub async fn fetch_reit_ffo_data_batch_to_cache(
    issuers: impl IntoIterator<Item = (impl AsRef<str>, impl AsRef<str>)>,
) -> Result<Vec<ReitFfoData>> {
    let mut results = Vec::new();
    for (symbol, cik) in issuers {
        results.push(fetch_reit_ffo_data_to_cache(symbol.as_ref(), cik.as_ref()).await?);
    }
    Ok(results)
}

fn issuer_source_dir(sources_dir: &Path, symbol: &str) -> PathBuf {
    sources_dir.join(symbol)
}

fn read_processed_accessions(path: &Path) -> Result<BTreeSet<String>> {
    if !path.is_file() {
        return Ok(BTreeSet::new());
    }
    let bytes = fs::read(path).with_context(|| format!("failed to read {}", path.display()))?;
    serde_json::from_slice(&bytes).with_context(|| format!("failed to parse {}", path.display()))
}

fn mark_accession_processed(
    path: &Path,
    processed_accessions: &mut BTreeSet<String>,
    accession: String,
) -> Result<()> {
    processed_accessions.insert(accession);
    write_json_atomic(path, processed_accessions)
}

fn document_mentions_ffo(
    source: &FfoSourceDocument,
    bytes: &[u8],
    content_type: Option<&str>,
) -> bool {
    let is_pdf = bytes.starts_with(b"%PDF-")
        || content_type.is_some_and(|value| value.to_ascii_lowercase().contains("pdf"));
    if is_pdf {
        let metadata = format!("{} {}", source.description, source.url).to_ascii_lowercase();
        return [
            "earnings",
            "financial results",
            "supplement",
            "funds from operations",
            "ffo",
            "affo",
        ]
        .iter()
        .any(|keyword| metadata.contains(keyword));
    }
    extract::contains_ffo_metric(&String::from_utf8_lossy(bytes))
}

fn assert_unique_source_file_names(
    accession: &str,
    documents: &[(FfoSourceDocument, Vec<u8>, Option<String>)],
) {
    let mut names = HashSet::new();
    for (source, _, _) in documents {
        let name = source_file_name(&source.url);
        assert!(
            names.insert(name.to_ascii_lowercase()),
            "FFO source filename collision for {name} in filing {accession}"
        );
    }
}

fn semantic_filing_name(
    filing: &crate::common::sec_api::SecFiling,
    documents: &[(FfoSourceDocument, Vec<u8>, Option<String>)],
) -> String {
    let form = filing.form.strip_suffix("/A").unwrap_or(&filing.form);
    let period = if matches!(form, "10-Q" | "10-K") {
        filing
            .report_date
            .as_deref()
            .and_then(calendar_quarter)
            .unwrap_or_else(|| filing.filing_date.clone())
    } else {
        filing
            .report_date
            .clone()
            .unwrap_or_else(|| filing.filing_date.clone())
    };
    let document_metadata = documents
        .iter()
        .map(|(source, _, _)| format!("{} {}", source.description, source.url))
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase();
    let kind = match form {
        "10-Q" => "10-q",
        "10-K" => "10-k",
        _ if document_metadata.contains("supplement")
            && !document_metadata.contains("earnings")
            && !document_metadata.contains("financial results") =>
        {
            "supplemental"
        }
        _ => "results",
    };
    let amended = filing.form.ends_with("/A")
        || document_metadata.contains("amend")
        || (kind == "supplemental"
            && documents.iter().any(|(_, bytes, _)| {
                String::from_utf8_lossy(bytes)
                    .to_ascii_lowercase()
                    .contains("amended and restated")
            }));
    format!("{period}-{kind}{}", if amended { "-amendment" } else { "" })
}

fn calendar_quarter(date: &str) -> Option<String> {
    let year = date.get(..4)?;
    let month = date.get(5..7)?.parse::<u8>().ok()?;
    let quarter = (month.checked_sub(1)? / 3) + 1;
    Some(format!("{year}-q{quarter}"))
}

pub(super) fn write_bytes_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let file_name = path
        .file_name()
        .context("download path has no file name")?
        .to_string_lossy();
    let temporary_path = path.with_file_name(format!(".{file_name}.tmp"));
    fs::write(&temporary_path, bytes)
        .with_context(|| format!("failed to write {}", temporary_path.display()))?;
    if let Err(error) = fs::rename(&temporary_path, path) {
        let _ = fs::remove_file(&temporary_path);
        return Err(error).with_context(|| format!("failed to replace {}", path.display()));
    }
    Ok(())
}

pub(super) fn source_file_name(url: &str) -> String {
    Url::parse(url)
        .ok()
        .and_then(|url| url.path_segments()?.next_back().map(str::to_owned))
        .filter(|name| !name.is_empty())
        .map(|name| sanitize_path_component(&name))
        .unwrap_or_else(|| "document".to_owned())
}

pub(super) fn sanitize_path_component(value: &str) -> String {
    let sanitized = value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '.' | '-' | '_') {
                character
            } else {
                '_'
            }
        })
        .collect::<String>();
    if sanitized.is_empty() {
        "document".to_owned()
    } else {
        sanitized
    }
}

#[cfg(test)]
use crate::common::sec_api::SecFiling;
#[cfg(test)]
use discovery::{filing_priority, is_likely_ffo_source, parse_filing_index, resolve_document_url};
#[cfg(test)]
use extract::{
    combined_numeric_cell, contains_ffo_metric, extract_values_from_html, normalize_number
};

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

    #[test]
    fn extracts_typed_values_complete_periods_and_reconciliation() {
        let html = r#"
            <html><body>
              <p>We define Core FFO as NAREIT FFO excluding transaction costs.</p>
              <p>Amounts in thousands, except per share data.</p>
              <table>
                <tr><th></th><th>Three Months Ended March 31, 2026</th><th>Three Months Ended March 31, 2025</th></tr>
                <tr><td>Net income attributable to common shareholders</td><td>$10,000</td><td>($2,000)</td></tr>
                <tr><td>Real estate depreciation</td><td>20,000</td><td>18,000</td></tr>
                <tr><td>Core FFO attributable to common shareholders</td><td>$30,000</td><td>$16,000</td></tr>
                <tr><td>Core FFO per diluted share</td><td>$1.25</td><td>$0.70</td></tr>
              </table>
            </body></html>
        "#;
        let source = FfoSourceDocument {
            url: "https://www.sec.gov/Archives/core-ffo.htm".to_owned(),
            exhibit_type: "EX-99.1".to_owned(),
            description: "Earnings release".to_owned(),
            filing_date: "2026-05-01".to_owned(),
            accession_number: "0000123-26-000001".to_owned(),
            filing_form: "8-K".to_owned(),
            filing_index_url: "https://www.sec.gov/Archives/index.html".to_owned(),
        };

        let values = extract_values_from_html(html, &source);

        assert_eq!(values.len(), 4);
        assert_eq!(values[0].value_type, "total");
        assert_eq!(values[0].value, 30_000.0);
        assert_eq!(
            values[0].reporting_period,
            "Three Months Ended March 31, 2026"
        );
        assert_eq!(values[0].units.as_deref(), Some("thousands"));
        assert_eq!(values[0].reconciliation.len(), 3);
        assert_eq!(values[2].value_type, "perShare");
        assert_eq!(values[2].value, 1.25);
    }

    #[test]
    fn normalizes_accounting_numbers_without_inventing_values() {
        assert_eq!(normalize_number("($1,234.50)"), Some(-1234.50));
        assert_eq!(
            combined_numeric_cell(
                &[
                    "Core FFO".to_owned(),
                    "(".to_owned(),
                    "1,234".to_owned(),
                    ")".to_owned(),
                ],
                2,
            ),
            "(1,234)"
        );
        assert_eq!(normalize_number("—"), None);
        assert_eq!(normalize_number("NAREIT"), None);
        assert!(!contains_ffo_metric("affordable housing"));
        assert!(contains_ffo_metric("Adjusted FFO per share"));
    }

    #[test]
    fn uses_stable_sec_document_names() {
        assert_eq!(
            source_file_name("https://www.sec.gov/Archives/edgar/data/123/1/earnings-release.htm"),
            "earnings-release.htm"
        );
        assert_eq!(
            source_file_name(
                "https://www.sec.gov/Archives/edgar/data/123/1/earnings-release.htm?source=ix"
            ),
            "earnings-release.htm"
        );
    }

    #[test]
    fn recognizes_ffo_documents_without_matching_unrelated_words() {
        let source = FfoSourceDocument {
            url: "https://www.sec.gov/results.htm".to_owned(),
            exhibit_type: "EX-99.1".to_owned(),
            description: "Results".to_owned(),
            filing_date: "2026-02-03".to_owned(),
            accession_number: "0000123-26-000001".to_owned(),
            filing_form: "8-K".to_owned(),
            filing_index_url: "https://www.sec.gov/index.html".to_owned(),
        };
        assert!(document_mentions_ffo(
            &source,
            b"<p>Funds From Operations attributable to shareholders</p>",
            Some("text/html")
        ));
        assert!(document_mentions_ffo(
            &source,
            b"<td>Adjusted FFO</td>",
            Some("text/html")
        ));
        assert!(!document_mentions_ffo(
            &source,
            b"<p>Affordable housing portfolio update</p>",
            Some("text/html")
        ));

        let supplemental = FfoSourceDocument {
            url: "https://www.sec.gov/q42025supplemental.pdf".to_owned(),
            description: "Quarterly supplemental".to_owned(),
            ..source
        };
        assert!(document_mentions_ffo(
            &supplemental,
            b"%PDF-compressed",
            Some("application/pdf")
        ));
    }

    #[test]
    fn uses_sec_dates_for_results_and_periodic_report_directories() {
        let mut eight_k = filing("8-K", &["2.02"], "form8-k.htm");
        eight_k.filing_date = "2026-02-03".to_owned();
        eight_k.report_date = Some("2026-02-03".to_owned());
        let source = FfoSourceDocument {
            url: "https://www.sec.gov/a4q25earningsrelease.htm".to_owned(),
            exhibit_type: "EX-99.1".to_owned(),
            description: "Earnings release".to_owned(),
            filing_date: eight_k.filing_date.clone(),
            accession_number: eight_k.accession_number.clone(),
            filing_form: eight_k.form.clone(),
            filing_index_url: eight_k.filing_index_url.clone(),
        };
        let documents = vec![(source, Vec::new(), Some("text/html".to_owned()))];

        assert_eq!(
            semantic_filing_name(&eight_k, &documents),
            "2026-02-03-results"
        );

        let mut ten_k = filing("10-K", &[], "annual.htm");
        ten_k.report_date = Some("2025-12-31".to_owned());
        assert_eq!(semantic_filing_name(&ten_k, &documents), "2025-q4-10-k");

        eight_k.report_date = None;
        eight_k.filing_date = "2026-02-04".to_owned();
        assert_eq!(
            semantic_filing_name(&eight_k, &documents),
            "2026-02-04-results"
        );
    }

    #[test]
    fn identifies_amended_supplemental_from_document_content() {
        let eight_k = filing("8-K", &["2.02"], "form8-k.htm");
        let source = FfoSourceDocument {
            url: "https://www.sec.gov/a4q25supplemental.htm".to_owned(),
            exhibit_type: "EX-99.1".to_owned(),
            description: "Supplemental information".to_owned(),
            filing_date: eight_k.filing_date.clone(),
            accession_number: eight_k.accession_number.clone(),
            filing_form: eight_k.form.clone(),
            filing_index_url: eight_k.filing_index_url.clone(),
        };
        let documents = vec![(
            source,
            b"Fourth Quarter 2025 Amended and Restated Supplemental Information".to_vec(),
            Some("text/html".to_owned()),
        )];

        assert_eq!(
            semantic_filing_name(&eight_k, &documents),
            "2026-03-31-supplemental-amendment"
        );
    }

    #[test]
    fn processed_filings_serialize_as_a_plain_sorted_array() {
        let processed = BTreeSet::from([
            "0001500217-26-000029".to_owned(),
            "0001500217-26-000004".to_owned(),
        ]);

        assert_eq!(
            serde_json::to_value(processed).unwrap(),
            serde_json::json!(["0001500217-26-000004", "0001500217-26-000029"])
        );
    }

    #[test]
    #[should_panic(expected = "FFO source filename collision")]
    fn panics_on_case_insensitive_source_filename_collisions() {
        let make_source = |url: &str| FfoSourceDocument {
            url: url.to_owned(),
            exhibit_type: "EX-99.1".to_owned(),
            description: "Earnings release".to_owned(),
            filing_date: "2026-02-03".to_owned(),
            accession_number: "0000123-26-000001".to_owned(),
            filing_form: "8-K".to_owned(),
            filing_index_url: "https://www.sec.gov/index.html".to_owned(),
        };
        let documents = vec![
            (
                make_source("https://www.sec.gov/Results.htm"),
                Vec::new(),
                None,
            ),
            (
                make_source("https://www.sec.gov/results.htm"),
                Vec::new(),
                None,
            ),
        ];

        assert_unique_source_file_names("0000123-26-000001", &documents);
    }

    #[test]
    fn allows_source_extensions_to_share_a_stem() {
        let make_source = |url: &str| FfoSourceDocument {
            url: url.to_owned(),
            exhibit_type: "EX-99.1".to_owned(),
            description: "Earnings release".to_owned(),
            filing_date: "2026-02-03".to_owned(),
            accession_number: "0000123-26-000001".to_owned(),
            filing_form: "8-K".to_owned(),
            filing_index_url: "https://www.sec.gov/index.html".to_owned(),
        };
        let documents = vec![
            (
                make_source("https://www.sec.gov/results.htm"),
                Vec::new(),
                None,
            ),
            (
                make_source("https://www.sec.gov/results.pdf"),
                Vec::new(),
                None,
            ),
        ];

        assert_unique_source_file_names("0000123-26-000001", &documents);
    }

    #[test]
    fn symbol_keys_the_sec_source_directory() {
        assert_eq!(
            issuer_source_dir(Path::new("data/ffo/sources"), "VICI"),
            Path::new("data/ffo/sources/VICI")
        );
    }
}
