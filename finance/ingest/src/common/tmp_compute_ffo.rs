//! Temporary pipeline for locating, archiving, and extracting REIT FFO/AFFO source documents.
//!
//! Extraction deliberately retains issuer terminology and raw table cells. FFO and AFFO are
//! non-GAAP measures whose definitions vary, so downstream code must not silently combine values
//! that have different labels or reconciliation methodologies.

use std::{collections::HashSet, fs, path::Path, process::Command};

use anyhow::{Context, Result};
use reqwest::Url;
use scraper::{ElementRef, Html, Selector};
use serde::Serialize;

use crate::{
    file_utils::write_json_atomic, meta_utils::{YieldWatchError, get_app_data_path}
};

use super::sec_api::{SecFiling, fetch_recent_filings, fetch_sec_document, fetch_sec_text};

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

/// A source location that makes an extracted value traceable to both the immutable SEC URL and
/// the archived local copy.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FfoValueSource {
    pub document_url: String,
    pub local_path: String,
    pub filing_index_url: String,
    pub accession_number: String,
    pub filing_form: String,
    pub filing_date: String,
    pub table_index: Option<usize>,
    pub row_index: Option<usize>,
}

/// One row in the issuer's reconciliation, retained verbatim after whitespace normalization.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FfoReconciliationRow {
    pub label: String,
    pub cells: Vec<String>,
}

/// A reported FFO/AFFO total or per-share amount.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportedFfoValue {
    /// Broad measure family (`ffo` or `affo`). See `companyLabel` for qualified variants such as
    /// Core FFO or Normalized FFO; the broad family never replaces that exact issuer label.
    pub metric: String,
    /// The issuer's exact row/line label; this is the authoritative company-specific methodology
    /// name and must be used when comparing values.
    pub company_label: String,
    /// `total` or `perShare`.
    pub value_type: String,
    /// Exact source spelling, including parentheses used to denote negative amounts.
    pub raw_value: String,
    /// A machine-friendly decimal string, without currency signs or grouping commas.
    pub normalized_value: String,
    pub reporting_period: Option<String>,
    pub units: Option<String>,
    /// Exact definition paragraphs found in the source document.
    pub definitions: Vec<String>,
    /// Rows from the nearest net-income starting point through the reported measure. This retains
    /// the issuer's exact calculation rather than imposing a universal FFO formula.
    pub reconciliation: Vec<FfoReconciliationRow>,
    pub source: FfoValueSource,
}

/// Archive and extraction result for one downloaded source document.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtractedFfoDocument {
    #[serde(flatten)]
    pub document: FfoSourceDocument,
    pub local_path: String,
    pub content_type: Option<String>,
    pub byte_length: usize,
    /// `extracted`, `noFfoValuesFound`, or `pdfTextUnavailable`.
    pub extraction_status: String,
    pub extraction_note: Option<String>,
    pub definitions: Vec<String>,
    pub values: Vec<ReportedFfoValue>,
}

/// Complete persisted result for an issuer.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReitFfoExtraction {
    pub cik: String,
    pub documents: Vec<ExtractedFfoDocument>,
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

/// Discovers, downloads, and extracts an issuer's recent FFO/AFFO source documents.
///
/// Raw documents are archived below `output_dir/<cik>/<accession>/`. The returned result is also
/// atomically written to `output_dir/<cik>/extraction.json`. Existing documents are replaced only
/// after a successful download.
#[allow(dead_code)]
pub async fn fetch_reit_ffo_data(
    cik: &str,
    output_dir: impl AsRef<Path>,
) -> Result<ReitFfoExtraction> {
    let sources = fetch_reit_ffo_sources(cik).await?;
    let issuer_dir = output_dir.as_ref().join(&sources.cik);
    fs::create_dir_all(&issuer_dir)
        .with_context(|| format!("failed to create {}", issuer_dir.display()))?;

    let mut documents = Vec::with_capacity(sources.documents.len());
    for (document_index, source) in sources.documents.into_iter().enumerate() {
        let (bytes, content_type) = fetch_sec_document(&source.url)
            .await
            .with_context(|| format!("failed to download {}", source.url))?;
        let accession = sanitize_path_component(&source.accession_number);
        let document_dir = issuer_dir.join(accession);
        fs::create_dir_all(&document_dir)
            .with_context(|| format!("failed to create {}", document_dir.display()))?;
        let file_name = source_file_name(&source.url, document_index, &content_type);
        let local_path = document_dir.join(file_name);
        write_bytes_atomic(&local_path, &bytes)?;

        documents.push(extract_downloaded_document(
            source,
            &local_path,
            content_type,
            bytes.len(),
            &bytes,
        ));
    }

    let extraction = ReitFfoExtraction {
        cik: sources.cik,
        documents,
    };
    write_json_atomic(&issuer_dir.join("extraction.json"), &extraction)?;
    Ok(extraction)
}

/// Uses the repository's temporary data directory (`data/tmp/ffo` in the default setup).
#[allow(dead_code)]
pub async fn fetch_reit_ffo_data_to_tmp(cik: &str) -> Result<ReitFfoExtraction> {
    fetch_reit_ffo_data(cik, get_app_data_path().join("tmp").join("ffo")).await
}

/// Sequential batch variant that honors the SEC fair-access request rate and caller CIK order.
#[allow(dead_code)]
pub async fn fetch_reit_ffo_data_batch_to_tmp(
    ciks: impl IntoIterator<Item = impl AsRef<str>>,
) -> Result<Vec<ReitFfoExtraction>> {
    let mut results = Vec::new();
    for cik in ciks {
        results.push(fetch_reit_ffo_data_to_tmp(cik.as_ref()).await?);
    }
    Ok(results)
}

fn write_bytes_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
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

fn source_file_name(url: &str, index: usize, content_type: &Option<String>) -> String {
    let from_url = Url::parse(url)
        .ok()
        .and_then(|url| url.path_segments()?.next_back().map(str::to_owned))
        .filter(|name| !name.is_empty())
        .map(|name| sanitize_path_component(&name));
    let name = from_url.unwrap_or_else(|| {
        let extension = if content_type
            .as_deref()
            .is_some_and(|value| value.contains("pdf"))
        {
            "pdf"
        } else {
            "html"
        };
        format!("document.{extension}")
    });
    format!("{index:02}-{name}")
}

fn sanitize_path_component(value: &str) -> String {
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

fn extract_downloaded_document(
    document: FfoSourceDocument,
    local_path: &Path,
    content_type: Option<String>,
    byte_length: usize,
    bytes: &[u8],
) -> ExtractedFfoDocument {
    let is_pdf = bytes.starts_with(b"%PDF-")
        || content_type
            .as_deref()
            .is_some_and(|value| value.contains("pdf"))
        || local_path
            .extension()
            .is_some_and(|value| value.eq_ignore_ascii_case("pdf"));

    let (definitions, values, extraction_note) = if is_pdf {
        match extract_pdf_text(local_path) {
            Ok(text) => {
                let definitions = extract_definitions_from_lines(&text);
                let values = extract_values_from_lines(&text, &document, local_path, &definitions);
                (definitions, values, None)
            }
            Err(error) => (Vec::new(), Vec::new(), Some(error.to_string())),
        }
    } else {
        let text = String::from_utf8_lossy(bytes);
        let (definitions, values) = extract_values_from_html(&text, &document, local_path);
        (definitions, values, None)
    };
    let extraction_status = if extraction_note.is_some() {
        "pdfTextUnavailable"
    } else if values.is_empty() {
        "noFfoValuesFound"
    } else {
        "extracted"
    }
    .to_owned();

    ExtractedFfoDocument {
        document,
        local_path: local_path.to_string_lossy().into_owned(),
        content_type,
        byte_length,
        extraction_status,
        extraction_note,
        definitions,
        values,
    }
}

fn extract_pdf_text(path: &Path) -> Result<String> {
    let output = Command::new("pdftotext")
        .args(["-layout"])
        .arg(path)
        .arg("-")
        .output()
        .context("pdftotext is required to extract PDF supplemental reports")?;
    if !output.status.success() {
        anyhow::bail!(
            "pdftotext failed for {}: {}",
            path.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

#[derive(Debug)]
struct ExtractedTableRow {
    cells: Vec<String>,
    is_header: bool,
}

fn extract_values_from_html(
    html: &str,
    document: &FfoSourceDocument,
    local_path: &Path,
) -> (Vec<String>, Vec<ReportedFfoValue>) {
    let parsed = Html::parse_document(html);
    let definition_selector = Selector::parse("p, li, div").expect("valid definition selector");
    let definitions = unique_strings(
        parsed
            .select(&definition_selector)
            .map(|element| normalized_text(element.text()))
            .filter(|text| text.len() <= 4_000)
            .filter(|text| is_definition_text(text)),
    );
    let table_selector = Selector::parse("table").expect("valid table selector");
    let row_selector = Selector::parse("tr").expect("valid table row selector");
    let cell_selector = Selector::parse("th, td").expect("valid table cell selector");
    let document_units = detect_units(&normalized_text(parsed.root_element().text()));
    let mut values = Vec::new();

    for (table_index, table) in parsed.select(&table_selector).enumerate() {
        let rows = table
            .select(&row_selector)
            .filter_map(|row| parse_table_row(row, &cell_selector))
            .collect::<Vec<_>>();
        let table_text = rows
            .iter()
            .flat_map(|row| row.cells.iter().map(String::as_str))
            .collect::<Vec<_>>()
            .join(" ");
        let units = detect_units(&table_text).or_else(|| document_units.clone());
        let headers = column_headers(&rows);

        for (row_index, row) in rows.iter().enumerate() {
            let Some(label_index) = row.cells.iter().position(|cell| contains_ffo_metric(cell))
            else {
                continue;
            };
            let company_label = row.cells[label_index].clone();
            let value_type = if is_per_share_label(&company_label) {
                "perShare"
            } else {
                "total"
            };
            let reconciliation = reconciliation_rows(&rows, row_index);
            for column_index in (label_index + 1)..row.cells.len() {
                let raw_value = combined_numeric_cell(&row.cells, column_index);
                let Some(normalized_value) = normalize_number(&raw_value) else {
                    continue;
                };
                values.push(ReportedFfoValue {
                    metric: metric_name(&company_label).to_owned(),
                    company_label: company_label.clone(),
                    value_type: value_type.to_owned(),
                    raw_value,
                    normalized_value,
                    reporting_period: headers.get(column_index).cloned().filter(|v| !v.is_empty()),
                    units: if value_type == "perShare" {
                        Some("currency per share".to_owned())
                    } else {
                        units.clone()
                    },
                    definitions: definitions.clone(),
                    reconciliation: reconciliation.clone(),
                    source: value_source(document, local_path, Some(table_index), Some(row_index)),
                });
            }
        }
    }

    (definitions, values)
}

fn parse_table_row(row: ElementRef<'_>, cell_selector: &Selector) -> Option<ExtractedTableRow> {
    let cells = row.select(cell_selector).collect::<Vec<_>>();
    if cells.is_empty() {
        return None;
    }
    Some(ExtractedTableRow {
        is_header: cells.iter().any(|cell| cell.value().name() == "th"),
        cells: cells
            .into_iter()
            .flat_map(|cell| {
                let text = normalized_text(cell.text());
                let span = cell
                    .value()
                    .attr("colspan")
                    .and_then(|value| value.parse::<usize>().ok())
                    .unwrap_or(1)
                    .clamp(1, 32);
                std::iter::repeat_n(text, span)
            })
            .collect(),
    })
}

fn column_headers(rows: &[ExtractedTableRow]) -> Vec<String> {
    let width = rows.iter().map(|row| row.cells.len()).max().unwrap_or(0);
    let mut headers = vec![String::new(); width];
    for row in rows {
        let row_text = row.cells.join(" ");
        let non_label_cells_are_headers = row.cells.iter().skip(1).all(|cell| {
            normalize_number(cell).is_none()
                || (cell.len() == 4 && cell.starts_with("20") && cell.parse::<u16>().is_ok())
        });
        if is_period_text(&row_text) || (row.is_header && non_label_cells_are_headers) {
            for (index, cell) in row.cells.iter().enumerate() {
                if index > 0 && !cell.is_empty() {
                    headers[index] = if headers[index].is_empty() {
                        cell.clone()
                    } else {
                        format!("{} | {}", headers[index], cell)
                    };
                }
            }
        }
    }
    headers
}

fn combined_numeric_cell(cells: &[String], index: usize) -> String {
    let value = &cells[index];
    if normalize_number(value).is_none() {
        return value.clone();
    }
    let prefix = cells
        .get(index.wrapping_sub(1))
        .map(String::as_str)
        .filter(|value| matches!(*value, "$" | "("))
        .unwrap_or("");
    let suffix = cells
        .get(index + 1)
        .map(String::as_str)
        .filter(|value| *value == ")")
        .unwrap_or("");
    format!("{prefix}{value}{suffix}")
}

fn reconciliation_rows(rows: &[ExtractedTableRow], metric_row: usize) -> Vec<FfoReconciliationRow> {
    let metric_is_affo = metric_name(&rows[metric_row].cells.join(" ")) == "affo";
    let start = (0..=metric_row)
        .rev()
        .find(|index| {
            let label = rows[*index].cells.first().map(String::as_str).unwrap_or("");
            let lower = label.to_ascii_lowercase();
            lower.contains("net income")
                || lower.contains("net loss")
                || lower.contains("net (loss)")
                || (metric_is_affo && metric_name(&lower) == "ffo")
        })
        .unwrap_or(metric_row);
    rows[start..=metric_row]
        .iter()
        .filter(|row| !row.is_header && row.cells.iter().any(|cell| !cell.is_empty()))
        .map(|row| FfoReconciliationRow {
            label: row.cells.first().cloned().unwrap_or_default(),
            cells: row.cells.clone(),
        })
        .collect()
}

fn extract_definitions_from_lines(text: &str) -> Vec<String> {
    unique_strings(
        text.split("\n\n")
            .map(|paragraph| paragraph.split_whitespace().collect::<Vec<_>>().join(" "))
            .filter(|paragraph| is_definition_text(paragraph)),
    )
}

fn extract_values_from_lines(
    text: &str,
    document: &FfoSourceDocument,
    local_path: &Path,
    definitions: &[String],
) -> Vec<ReportedFfoValue> {
    let lines = text
        .lines()
        .map(|line| line.split_whitespace().collect::<Vec<_>>().join(" "))
        .collect::<Vec<_>>();
    let units = detect_units(text);
    let mut values = Vec::new();
    for (line_index, line) in lines.iter().enumerate() {
        if !contains_ffo_metric(line) {
            continue;
        }
        let period = lines[..line_index]
            .iter()
            .rev()
            .take(12)
            .find(|line| is_period_text(line))
            .cloned();
        let metric_is_affo = metric_name(line) == "affo";
        let start = lines[..=line_index]
            .iter()
            .rposition(|line| {
                let lower = line.to_ascii_lowercase();
                lower.contains("net income")
                    || lower.contains("net loss")
                    || lower.contains("net (loss)")
                    || (metric_is_affo && metric_name(&lower) == "ffo")
            })
            .unwrap_or(line_index);
        let reconciliation = lines[start..=line_index]
            .iter()
            .filter(|line| !line.is_empty())
            .map(|line| FfoReconciliationRow {
                label: line.clone(),
                cells: numeric_tokens(line),
            })
            .collect::<Vec<_>>();
        let value_type = if is_per_share_label(line) {
            "perShare"
        } else {
            "total"
        };

        for raw_value in numeric_tokens(line) {
            let Some(normalized_value) = normalize_number(&raw_value) else {
                continue;
            };
            values.push(ReportedFfoValue {
                metric: metric_name(line).to_owned(),
                company_label: line.clone(),
                value_type: value_type.to_owned(),
                raw_value,
                normalized_value,
                reporting_period: period.clone(),
                units: if value_type == "perShare" {
                    Some("currency per share".to_owned())
                } else {
                    units.clone()
                },
                definitions: definitions.to_vec(),
                reconciliation: reconciliation.clone(),
                source: value_source(document, local_path, None, Some(line_index)),
            });
        }
    }
    values
}

fn numeric_tokens(text: &str) -> Vec<String> {
    text.split_whitespace()
        .map(|token| token.trim_matches(|character: char| matches!(character, ':' | ';' | '*')))
        .filter(|token| normalize_number(token).is_some())
        .map(str::to_owned)
        .collect()
}

fn normalize_number(raw: &str) -> Option<String> {
    let mut value = raw.trim();
    if value.is_empty() || matches!(value, "-" | "—" | "–") {
        return None;
    }
    let negative = value.starts_with('(') && value.ends_with(')');
    if negative {
        value = &value[1..value.len() - 1];
    }
    value = value.trim().trim_start_matches('$').trim_end_matches('%');
    let normalized = value.replace([',', ' '], "");
    if normalized.is_empty()
        || normalized
            .chars()
            .any(|character| !character.is_ascii_digit() && character != '.' && character != '-')
        || normalized.parse::<f64>().is_err()
    {
        return None;
    }
    Some(if negative {
        format!("-{normalized}")
    } else {
        normalized
    })
}

fn contains_ffo_metric(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    lower.contains("funds from operations")
        || lower
            .split(|character: char| !character.is_ascii_alphanumeric())
            .any(|word| matches!(word, "ffo" | "affo"))
}

fn metric_name(label: &str) -> &'static str {
    let words = label
        .to_ascii_lowercase()
        .split(|character: char| !character.is_ascii_alphanumeric())
        .map(str::to_owned)
        .collect::<Vec<_>>();
    if words.iter().any(|word| word == "affo") {
        "affo"
    } else if words.iter().any(|word| word == "ffo")
        || label.to_ascii_lowercase().contains("funds from operations")
    {
        "ffo"
    } else {
        // `contains_ffo_metric` guards normal call sites; keep unusual punctuation in the broad
        // FFO family while preserving the authoritative issuer wording in `company_label`.
        "ffo"
    }
}

fn is_per_share_label(label: &str) -> bool {
    let lower = label.to_ascii_lowercase();
    lower.contains("per share")
        || lower.contains("per diluted share")
        || lower.contains("per basic share")
}

fn is_definition_text(text: &str) -> bool {
    if !contains_ffo_metric(text) {
        return false;
    }
    let lower = text.to_ascii_lowercase();
    [
        "define",
        "definition",
        "means",
        "calculated",
        "reconcile",
        "non-gaap",
    ]
    .iter()
    .any(|cue| lower.contains(cue))
}

fn is_period_text(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    [
        "three months",
        "six months",
        "nine months",
        "twelve months",
        "quarter ended",
        "year ended",
    ]
    .iter()
    .any(|cue| lower.contains(cue))
        || [
            "january",
            "february",
            "march",
            "april",
            "may",
            "june",
            "july",
            "august",
            "september",
            "october",
            "november",
            "december",
        ]
        .iter()
        .any(|month| lower.contains(month))
}

fn detect_units(text: &str) -> Option<String> {
    let lower = text.to_ascii_lowercase();
    if lower.contains("in millions") || lower.contains("amounts in millions") {
        Some("currency in millions".to_owned())
    } else if lower.contains("in thousands") || lower.contains("amounts in thousands") {
        Some("currency in thousands".to_owned())
    } else {
        None
    }
}

fn unique_strings(values: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut seen = HashSet::new();
    values
        .into_iter()
        .filter(|value| !value.is_empty() && seen.insert(value.clone()))
        .collect()
}

fn value_source(
    document: &FfoSourceDocument,
    local_path: &Path,
    table_index: Option<usize>,
    row_index: Option<usize>,
) -> FfoValueSource {
    FfoValueSource {
        document_url: document.url.clone(),
        local_path: local_path.to_string_lossy().into_owned(),
        filing_index_url: document.filing_index_url.clone(),
        accession_number: document.accession_number.clone(),
        filing_form: document.filing_form.clone(),
        filing_date: document.filing_date.clone(),
        table_index,
        row_index,
    }
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

        let (definitions, values) =
            extract_values_from_html(html, &source, Path::new("data/tmp/ffo/core-ffo.htm"));

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
}
