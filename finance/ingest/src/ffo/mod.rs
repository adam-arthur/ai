//! Pipeline for extracting canonical, period-oriented REIT FFO/AFFO actual results.
//!
//! Source documents are cached separately. Persisted issuer data contains only numeric measures
//! and parsed reconciliation components; HTML cells and other extraction internals never leak into
//! the consumer-facing model.

use std::{
    collections::{BTreeMap, BTreeSet}, fs, path::{Path, PathBuf}
};

use anyhow::{Context, Result};
use reqwest::Url;

mod candidate_tables;
mod canonical;
mod discovery;
mod models;
mod table_html;

use models::ExtractedFfoDocument;
pub(crate) use models::ffo_name_changes;
pub use models::{
    FfoAdjustment, FfoMeasure, FfoMeasures, FfoNameChange, FfoPeriodResult, FfoReconciliation, FfoReportingPeriod, FfoSourceDocument, ReitFfoData, ReitFfoSources
};

use crate::file_utils::write_json_atomic;
use crate::{
    common::sec_api::fetch_sec_document, financials::local_api::read_corporate_actions_from, meta_utils::get_app_data_path
};

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

/// Discovers and downloads an issuer's recent FFO/AFFO source documents.
///
/// Raw documents are archived byte-for-byte under
/// `data_dir/sec/filings/<symbol>/<filing-year>/<accession>/<SEC-filename>`. Derived candidate
/// tables live under `data_dir/ffo/derived/<symbol>/<report-quarter>/tables`. Documents examined
/// and found not to contain FFO material are listed by `<accession>/<SEC-filename>` in a
/// year-sharded JSON array.
#[allow(dead_code)]
pub async fn fetch_reit_ffo_data(
    symbol: &str,
    cik: &str,
    name_changes: Vec<FfoNameChange>,
    data_dir: impl AsRef<Path>,
) -> Result<ReitFfoData> {
    let data_dir = data_dir.as_ref();
    let filings = discovery::discover_reit_ffo_filings(cik).await?;
    let mut documents = Vec::new();
    let mut candidate_sources_by_period =
        BTreeMap::<String, Vec<(PathBuf, Option<String>, Vec<u8>)>>::new();
    for filing in filings {
        let accession = filing.filing.accession_number.clone();
        let filing_year = filing_year(&filing.filing)?;
        let raw_dir = raw_filing_dir(data_dir, symbol, &filing_year, &accession);
        let skip_path = skipped_files_path(data_dir, symbol, &filing_year);
        let mut skipped_files = read_skipped_files(&skip_path)?;
        let mut fetched_documents = Vec::new();
        for source in filing.documents {
            let file_name = source_file_name(&source.url);
            let skip_key = skipped_file_key(&accession, &file_name);
            if skipped_files.contains(&skip_key) {
                continue;
            }

            let raw_path = raw_dir.join(&file_name);
            let (bytes, content_type) = if raw_path.is_file() {
                (
                    fs::read(&raw_path)
                        .with_context(|| format!("failed to read {}", raw_path.display()))?,
                    None,
                )
            } else {
                let (bytes, content_type) = fetch_sec_document(&source.url)
                    .await
                    .with_context(|| format!("failed to download {}", source.url))?;
                if bytes.is_empty() {
                    anyhow::bail!("SEC returned an empty source document for {}", source.url);
                }
                if !document_mentions_ffo(&source, &bytes, content_type.as_deref()) {
                    skipped_files.insert(skip_key);
                    write_skipped_files(&skip_path, &skipped_files)?;
                    continue;
                }
                fs::create_dir_all(&raw_dir)
                    .with_context(|| format!("failed to create {}", raw_dir.display()))?;
                write_bytes_atomic(&raw_path, &bytes)?;
                (bytes, content_type)
            };

            fetched_documents.push((source, bytes, content_type));
        }

        if fetched_documents.is_empty() {
            continue;
        }

        let report_period = filing_calendar_quarter(&filing.filing)?;
        candidate_sources_by_period
            .entry(report_period)
            .or_default()
            .extend(
                fetched_documents
                    .iter()
                    .map(|(source, bytes, content_type)| {
                        (
                            raw_dir.join(source_file_name(&source.url)),
                            content_type.clone(),
                            bytes.clone(),
                        )
                    }),
            );

        // Value extraction will be supplied by the future LLM pipeline. Keep each relevant SEC
        // document in canonicalization input, but do not publish inferred values yet.
        documents.extend(fetched_documents.into_iter().map(|(source, _, _)| {
            ExtractedFfoDocument {
                document: source,
                values: Vec::new(),
            }
        }));
    }

    for (report_period, candidate_sources) in candidate_sources_by_period {
        let derived_dir = derived_quarter_dir(data_dir, symbol, &report_period);
        if derived_dir.join("tables").is_dir() {
            continue;
        }
        fs::create_dir_all(&derived_dir)
            .with_context(|| format!("failed to create {}", derived_dir.display()))?;
        let table_count =
            candidate_tables::ensure_candidate_tables(&derived_dir, candidate_sources)
                .with_context(|| {
                    format!("FFO table extraction failed for {}", derived_dir.display())
                })?;
        log::debug!(
            "FFO - cached {table_count} candidate table(s) in {}",
            derived_dir.join("tables").display()
        );
    }

    Ok(canonical::canonicalize(
        symbol,
        cik.trim_start_matches('0'),
        name_changes,
        documents,
    ))
}

/// Uses the repository data directory for immutable SEC filings and derived FFO artifacts.
#[allow(dead_code)]
pub async fn fetch_reit_ffo_data_to_cache(
    symbol: &str,
    cik: &str,
    name_changes: Vec<FfoNameChange>,
) -> Result<ReitFfoData> {
    fetch_reit_ffo_data(symbol, cik, name_changes, get_app_data_path()).await
}

/// Sequential batch variant that honors the SEC fair-access request rate and caller issuer order.
#[allow(dead_code)]
pub async fn fetch_reit_ffo_data_batch_to_cache(
    issuers: impl IntoIterator<Item = (impl AsRef<str>, impl AsRef<str>)>,
) -> Result<Vec<ReitFfoData>> {
    let mut results = Vec::new();
    for (symbol, cik) in issuers {
        let symbol = symbol.as_ref();
        let corporate_actions = read_corporate_actions_from(get_app_data_path(), symbol)?;
        let name_changes = ffo_name_changes(&corporate_actions);
        results.push(fetch_reit_ffo_data_to_cache(symbol, cik.as_ref(), name_changes).await?);
    }
    Ok(results)
}

fn raw_filing_dir(data_dir: &Path, symbol: &str, filing_year: &str, accession: &str) -> PathBuf {
    data_dir
        .join("sec")
        .join("filings")
        .join(symbol)
        .join(filing_year)
        .join(accession)
}

fn skipped_files_path(data_dir: &Path, symbol: &str, filing_year: &str) -> PathBuf {
    data_dir
        .join("sec")
        .join("cache")
        .join(symbol)
        .join(format!("{filing_year}.json"))
}

fn derived_quarter_dir(data_dir: &Path, symbol: &str, report_period: &str) -> PathBuf {
    data_dir
        .join("ffo")
        .join("derived")
        .join(symbol)
        .join(report_period)
}

fn read_skipped_files(path: &Path) -> Result<BTreeSet<String>> {
    if !path.is_file() {
        return Ok(BTreeSet::new());
    }
    let bytes = fs::read(path).with_context(|| format!("failed to read {}", path.display()))?;
    serde_json::from_slice(&bytes).with_context(|| format!("failed to parse {}", path.display()))
}

fn write_skipped_files(path: &Path, skipped_files: &BTreeSet<String>) -> Result<()> {
    let parent = path.parent().context("SEC skip-list path has no parent")?;
    fs::create_dir_all(parent).with_context(|| format!("failed to create {}", parent.display()))?;
    write_json_atomic(path, skipped_files)
}

fn skipped_file_key(accession: &str, file_name: &str) -> String {
    format!("{accession}/{file_name}")
}

fn filing_year(filing: &crate::common::sec_api::SecFiling) -> Result<String> {
    let year = filing
        .filing_date
        .get(..4)
        .filter(|year| year.len() == 4 && year.chars().all(|character| character.is_ascii_digit()));
    year.map(str::to_owned).with_context(|| {
        format!(
            "SEC filing {} has an invalid filing date: {}",
            filing.accession_number, filing.filing_date
        )
    })
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
    contains_ffo_metric(&String::from_utf8_lossy(bytes))
}

fn contains_ffo_metric(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    lower.contains("funds from operations")
        || lower
            .split(|character: char| !character.is_ascii_alphanumeric())
            .any(|word| matches!(word, "ffo" | "affo" | "nffo"))
}

fn filing_calendar_quarter(filing: &crate::common::sec_api::SecFiling) -> Result<String> {
    let date = filing.report_date.as_deref().unwrap_or(&filing.filing_date);
    calendar_quarter(date).with_context(|| {
        format!(
            "SEC filing {} has an invalid report/filing date: {date}",
            filing.accession_number
        )
    })
}

fn calendar_quarter(date: &str) -> Option<String> {
    let year = date.get(..4)?;
    let month = date.get(5..7)?.parse::<u8>().ok()?;
    if !year.chars().all(|character| character.is_ascii_digit()) || !(1..=12).contains(&month) {
        return None;
    }
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
        .unwrap_or_else(|| "document".to_owned())
}

#[cfg(test)]
use crate::common::sec_api::SecFiling;
#[cfg(test)]
use discovery::{filing_priority, is_likely_ffo_source, parse_filing_index, resolve_document_url};
#[cfg(test)]
mod tests {
    use super::*;
    use crate::ingest_utils::DATA_FETCH_START_DATE;

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
    fn limits_ffo_sources_to_the_centralized_data_fetch_period() {
        let mut before_cutoff = filing("10-K", &[], "annual.htm");
        before_cutoff.report_date = Some("2015-12-31".to_owned());
        let mut at_cutoff = before_cutoff.clone();
        at_cutoff.report_date = Some(DATA_FETCH_START_DATE.to_string());
        let mut filing_date_fallback = at_cutoff.clone();
        filing_date_fallback.report_date = None;
        filing_date_fallback.filing_date = DATA_FETCH_START_DATE.to_string();

        assert!(!discovery::is_within_data_fetch_period(&before_cutoff));
        assert!(discovery::is_within_data_fetch_period(&at_cutoff));
        assert!(discovery::is_within_data_fetch_period(
            &filing_date_fallback
        ));
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
    fn uses_report_date_then_filing_date_for_calendar_quarters() {
        let mut eight_k = filing("8-K", &["2.02"], "form8-k.htm");
        eight_k.filing_date = "2026-02-03".to_owned();
        eight_k.report_date = Some("2025-12-31".to_owned());
        assert_eq!(filing_calendar_quarter(&eight_k).unwrap(), "2025-q4");

        let mut ten_k = filing("10-K", &[], "annual.htm");
        ten_k.report_date = Some("2025-12-31".to_owned());
        assert_eq!(filing_calendar_quarter(&ten_k).unwrap(), "2025-q4");

        eight_k.report_date = None;
        eight_k.filing_date = "2026-05-04".to_owned();
        assert_eq!(filing_calendar_quarter(&eight_k).unwrap(), "2026-q2");
    }

    #[test]
    fn rejects_invalid_calendar_quarter_dates() {
        assert_eq!(calendar_quarter("2026-00-31"), None);
        assert_eq!(calendar_quarter("2026-13-01"), None);
        assert_eq!(calendar_quarter("year-03-31"), None);
    }

    #[test]
    fn skipped_files_serialize_as_a_plain_sorted_array() {
        let skipped = BTreeSet::from([
            "0001500217-26-000029/results.htm".to_owned(),
            "0001500217-26-000004/exhibit99.htm".to_owned(),
        ]);

        assert_eq!(
            serde_json::to_value(skipped).unwrap(),
            serde_json::json!([
                "0001500217-26-000004/exhibit99.htm",
                "0001500217-26-000029/results.htm"
            ])
        );
    }

    #[test]
    fn separates_raw_sec_filings_skip_lists_and_ffo_derivations() {
        let data_dir = Path::new("data");
        let accession = "0000123-26-000001";

        assert_eq!(
            raw_filing_dir(data_dir, "VICI", "2026", accession),
            Path::new("data/sec/filings/VICI/2026/0000123-26-000001")
        );
        assert_eq!(
            skipped_files_path(data_dir, "VICI", "2026"),
            Path::new("data/sec/cache/VICI/2026.json")
        );
        assert_eq!(
            derived_quarter_dir(data_dir, "VICI", "2025-q4"),
            Path::new("data/ffo/derived/VICI/2025-q4")
        );
        assert_eq!(
            skipped_file_key(accession, "earnings.htm"),
            "0000123-26-000001/earnings.htm"
        );
    }

    #[test]
    fn uses_filing_date_year_for_raw_sec_storage() {
        let mut filing = filing("8-K", &["2.02"], "form8-k.htm");
        filing.filing_date = "2026-02-03".to_owned();
        filing.report_date = Some("2025-12-31".to_owned());

        assert_eq!(filing_year(&filing).unwrap(), "2026");
    }
}
