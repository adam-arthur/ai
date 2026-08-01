//! Pipeline for locating, archiving, and extracting REIT FFO/AFFO source documents.
//!
//! Extraction deliberately retains issuer terminology and raw table cells. FFO and AFFO are
//! non-GAAP measures whose definitions vary, so downstream code must not silently combine values
//! that have different labels or reconciliation methodologies.

use std::{
    fs, path::{Path, PathBuf}
};

use anyhow::{Context, Result};
use reqwest::Url;

mod discovery;
mod extract;
mod models;

pub use models::{
    ExtractedFfoDocument, FfoReconciliationRow, FfoSourceDocument, FfoValueSource, ReitFfoExtraction, ReitFfoSources, ReportedFfoValue
};

use crate::{common::sec_api::fetch_sec_document, meta_utils::get_app_data_path};

/// Locates likely FFO/AFFO source documents without writing to the persistent source cache.
#[allow(dead_code)]
pub async fn fetch_reit_ffo_sources(cik: &str) -> Result<ReitFfoSources> {
    discovery::discover_reit_ffo_sources(cik, None).await
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
/// Raw documents are archived below `sources_dir/<symbol>/<accession>/<document-name>`. An
/// existing non-empty document is read from disk and is never fetched again. The CIK is used only
/// to query SEC data and retained in the returned extraction for provenance.
#[allow(dead_code)]
pub async fn fetch_reit_ffo_data(
    symbol: &str,
    cik: &str,
    sources_dir: impl AsRef<Path>,
) -> Result<ReitFfoExtraction> {
    let issuer_dir = issuer_source_dir(sources_dir.as_ref(), symbol);
    let sources = discovery::discover_reit_ffo_sources(cik, Some(&issuer_dir)).await?;
    fs::create_dir_all(&issuer_dir)
        .with_context(|| format!("failed to create {}", issuer_dir.display()))?;

    let mut documents = Vec::with_capacity(sources.documents.len());
    for source in sources.documents {
        let accession = sanitize_path_component(&source.accession_number);
        let document_dir = issuer_dir.join(accession);
        fs::create_dir_all(&document_dir)
            .with_context(|| format!("failed to create {}", document_dir.display()))?;
        let file_name = source_file_name(&source.url);
        let local_path = document_dir.join(file_name);
        let (bytes, content_type) = load_or_fetch_source_document(&source.url, &local_path).await?;

        documents.push(extract::extract_downloaded_document(
            source,
            &local_path,
            content_type,
            bytes.len(),
            &bytes,
        ));
    }

    Ok(ReitFfoExtraction {
        cik: sources.cik,
        documents,
    })
}

/// Uses the repository's persistent source cache (`data/ffo/sources` in the default setup).
#[allow(dead_code)]
pub async fn fetch_reit_ffo_data_to_cache(symbol: &str, cik: &str) -> Result<ReitFfoExtraction> {
    fetch_reit_ffo_data(symbol, cik, get_app_data_path().join("ffo").join("sources")).await
}

/// Sequential batch variant that honors the SEC fair-access request rate and caller issuer order.
#[allow(dead_code)]
pub async fn fetch_reit_ffo_data_batch_to_cache(
    issuers: impl IntoIterator<Item = (impl AsRef<str>, impl AsRef<str>)>,
) -> Result<Vec<ReitFfoExtraction>> {
    let mut results = Vec::new();
    for (symbol, cik) in issuers {
        results.push(fetch_reit_ffo_data_to_cache(symbol.as_ref(), cik.as_ref()).await?);
    }
    Ok(results)
}

fn issuer_source_dir(sources_dir: &Path, symbol: &str) -> PathBuf {
    sources_dir.join(symbol)
}

async fn load_or_fetch_source_document(
    url: &str,
    local_path: &Path,
) -> Result<(Vec<u8>, Option<String>)> {
    if let Some(bytes) = read_non_empty_file(local_path)? {
        log::debug!(
            "FFO source - using cached document {}",
            local_path.display()
        );
        let content_type = infer_content_type(local_path, &bytes);
        return Ok((bytes, content_type));
    }

    let (bytes, content_type) = fetch_sec_document(url)
        .await
        .with_context(|| format!("failed to download {url}"))?;
    if bytes.is_empty() {
        anyhow::bail!("SEC returned an empty source document for {url}");
    }
    write_bytes_atomic(local_path, &bytes)?;
    Ok((bytes, content_type))
}

pub(super) fn read_non_empty_file(path: &Path) -> Result<Option<Vec<u8>>> {
    if !path.is_file() {
        return Ok(None);
    }
    let bytes = fs::read(path).with_context(|| format!("failed to read {}", path.display()))?;
    Ok((!bytes.is_empty()).then_some(bytes))
}

fn infer_content_type(path: &Path, bytes: &[u8]) -> Option<String> {
    if bytes.starts_with(b"%PDF-")
        || path
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("pdf"))
    {
        Some("application/pdf".to_owned())
    } else if path.extension().is_some_and(|extension| {
        extension.eq_ignore_ascii_case("htm") || extension.eq_ignore_ascii_case("html")
    }) {
        Some("text/html".to_owned())
    } else {
        None
    }
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
    fn extracts_totals_per_share_periods_definitions_and_reconciliation() {
        let html = r#"
            <html><body>
              <p>We define Core FFO as NAREIT FFO excluding transaction costs.</p>
              <table>
                <tr><th></th><th>Three Months Ended March 31, 2026</th><th>Three Months Ended March 31, 2025</th></tr>
                <tr><td>Net income attributable to common shareholders</td><td>$10,000</td><td>($2,000)</td></tr>
                <tr><td>Real estate depreciation</td><td>20,000</td><td>18,000</td></tr>
                <tr><td>Core FFO attributable to common shareholders</td><td>$30,000</td><td>$16,000</td></tr>
                <tr><td>Core FFO per diluted share</td><td>$1.25</td><td>$0.70</td></tr>
              </table>
              <p>Amounts in thousands, except per share data.</p>
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

        let (definitions, values) = extract_values_from_html(
            html,
            &source,
            Path::new("data/ffo/sources/123/core-ffo.htm"),
        );

        assert_eq!(
            definitions,
            ["We define Core FFO as NAREIT FFO excluding transaction costs."]
        );
        assert_eq!(values.len(), 4);
        assert_eq!(values[0].value_type, "total");
        assert_eq!(values[0].raw_value, "$30,000");
        assert_eq!(values[0].normalized_value, "30000");
        assert_eq!(
            values[0].reporting_period.as_deref(),
            Some("Three Months Ended March 31, 2026")
        );
        assert_eq!(values[0].units.as_deref(), Some("currency in thousands"));
        assert_eq!(values[0].reconciliation.len(), 3);
        assert_eq!(values[2].value_type, "perShare");
        assert_eq!(values[2].normalized_value, "1.25");
        assert_eq!(values[2].source.table_index, Some(0));
        assert_eq!(values[2].source.row_index, Some(4));
    }

    #[test]
    fn normalizes_accounting_numbers_without_inventing_values() {
        assert_eq!(normalize_number("($1,234.50)"), Some("-1234.50".to_owned()));
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
    fn symbol_keys_the_sec_source_directory() {
        assert_eq!(
            issuer_source_dir(Path::new("data/ffo/sources"), "VICI"),
            Path::new("data/ffo/sources/VICI")
        );
    }

    #[test]
    fn reads_non_empty_cached_source_documents() {
        let cache_path = std::env::temp_dir().join(format!(
            "finance-ffo-cache-{}-{}.htm",
            std::process::id(),
            std::thread::current().name().unwrap_or("test")
        ));
        fs::write(&cache_path, b"<html>cached filing</html>").unwrap();

        assert_eq!(
            read_non_empty_file(&cache_path).unwrap(),
            Some(b"<html>cached filing</html>".to_vec())
        );
        assert_eq!(
            infer_content_type(&cache_path, b"<html>cached filing</html>").as_deref(),
            Some("text/html")
        );

        fs::remove_file(cache_path).unwrap();
    }
}
