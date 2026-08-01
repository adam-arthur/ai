use std::{collections::HashSet, path::Path, process::Command};

use anyhow::{Context, Result};
use scraper::{ElementRef, Html, Selector};

use super::{
    ExtractedFfoDocument, FfoReconciliationRow, FfoSourceDocument, FfoValueSource, ReportedFfoValue
};

fn normalized_text<'a>(fragments: impl Iterator<Item = &'a str>) -> String {
    fragments
        .collect::<Vec<_>>()
        .join(" ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

pub(super) fn extract_downloaded_document(
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

pub(super) fn extract_values_from_html(
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

pub(super) fn combined_numeric_cell(cells: &[String], index: usize) -> String {
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

pub(super) fn normalize_number(raw: &str) -> Option<String> {
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

pub(super) fn contains_ffo_metric(text: &str) -> bool {
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
