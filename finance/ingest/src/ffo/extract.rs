use std::{collections::HashSet, path::Path};

use scraper::{ElementRef, Html, Selector};

use super::models::{
    ExtractedFfoDocument, ExtractedReconciliationRow, FfoSourceDocument, ReportedFfoValue
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
    _local_path: &Path,
    content_type: Option<String>,
    _byte_length: usize,
    bytes: &[u8],
) -> ExtractedFfoDocument {
    let is_pdf = bytes.starts_with(b"%PDF-")
        || content_type
            .as_deref()
            .is_some_and(|value| value.contains("pdf"));

    // PDF text has no dependable row/column association. Omitting it is preferable to publishing
    // a number with the wrong year, unit, or measure. HTML exhibits cover the high-confidence path.
    let values = if is_pdf {
        Vec::new()
    } else {
        extract_values_from_html(&String::from_utf8_lossy(bytes), &document)
    };

    ExtractedFfoDocument { document, values }
}

#[derive(Clone, Debug)]
struct ExtractedTableRow {
    cells: Vec<String>,
    is_header: bool,
}

pub(super) fn extract_values_from_html(
    html: &str,
    _document: &FfoSourceDocument,
) -> Vec<ReportedFfoValue> {
    let parsed = Html::parse_document(html);
    let table_selector = Selector::parse("table").expect("valid table selector");
    let row_selector = Selector::parse("tr").expect("valid table row selector");
    let cell_selector = Selector::parse("th, td").expect("valid table cell selector");
    let contexts = table_contexts(&parsed);
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
        let context = contexts.get(table_index).map(String::as_str).unwrap_or("");
        if context.to_ascii_lowercase().contains("guidance") {
            continue;
        }
        let units = detect_units(context).or_else(|| detect_units(&table_text));
        let headers = column_headers(&rows);

        for (row_index, row) in rows.iter().enumerate() {
            let Some(label_index) = row.cells.iter().position(|cell| is_candidate_label(cell))
            else {
                continue;
            };
            let company_label = row.cells[label_index].clone();
            let is_shares = is_weighted_average_diluted_shares(&company_label);
            let variant = metric_variant(&company_label);
            if !is_shares && variant.is_none() {
                continue;
            }
            let value_type = if is_shares {
                "shares"
            } else if is_per_share_label(&company_label) {
                if company_label.to_ascii_lowercase().contains("basic") {
                    continue;
                }
                "perShare"
            } else {
                "total"
            };

            for column_index in (label_index + 1)..row.cells.len() {
                let Some(reporting_period) = headers
                    .get(column_index)
                    .filter(|header| parse_period(header).is_some())
                else {
                    continue;
                };
                let raw_value = combined_numeric_cell(&row.cells, column_index);
                let Some(value) = normalize_number(&raw_value) else {
                    continue;
                };
                values.push(ReportedFfoValue {
                    variant: variant.unwrap_or("shares").to_owned(),
                    company_label: company_label.clone(),
                    value_type: value_type.to_owned(),
                    value,
                    reporting_period: reporting_period.clone(),
                    units: if value_type == "perShare" {
                        Some("currency per share".to_owned())
                    } else {
                        units.clone()
                    },
                    reconciliation: if value_type == "total" {
                        reconciliation_rows(&rows, row_index, column_index)
                    } else {
                        Vec::new()
                    },
                });
            }
        }
    }

    values
}

fn table_contexts(parsed: &Html) -> Vec<String> {
    let selector = Selector::parse("p, table").expect("valid context selector");
    let mut recent_paragraphs = Vec::new();
    let mut contexts = Vec::new();
    for element in parsed.select(&selector) {
        if element.value().name() == "table" {
            contexts.push(recent_paragraphs.join(" "));
            continue;
        }
        let inside_table = element
            .ancestors()
            .filter_map(ElementRef::wrap)
            .any(|ancestor| ancestor.value().name() == "table");
        if inside_table {
            continue;
        }
        let text = normalized_text(element.text());
        if !text.is_empty() {
            recent_paragraphs.push(text);
            if recent_paragraphs.len() > 6 {
                recent_paragraphs.remove(0);
            }
        }
    }
    contexts
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
        let nonempty = row
            .cells
            .iter()
            .filter(|cell| !cell.is_empty())
            .collect::<Vec<_>>();
        let is_year_row =
            !nonempty.is_empty() && nonempty.iter().all(|cell| is_four_digit_year(cell));
        let non_label_cells_are_headers = row.cells.iter().skip(1).all(|cell| {
            cell.is_empty() || normalize_number(cell).is_none() || is_four_digit_year(cell)
        });
        if is_period_text(&row_text)
            || is_year_row
            || (row.is_header && non_label_cells_are_headers)
        {
            for (index, cell) in row.cells.iter().enumerate().skip(1) {
                if cell.is_empty() {
                    continue;
                }
                let parts = headers[index].split(" | ").collect::<HashSet<_>>();
                if !parts.contains(cell.as_str()) {
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

fn is_four_digit_year(value: &str) -> bool {
    value.len() == 4
        && value.starts_with("20")
        && value
            .parse::<u16>()
            .is_ok_and(|year| (2000..=2100).contains(&year))
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
        .filter(|value| value.trim().starts_with(')'))
        .map(|_| ")")
        .unwrap_or("");
    format!("{prefix}{value}{suffix}")
}

fn reconciliation_rows(
    rows: &[ExtractedTableRow],
    metric_row: usize,
    column_index: usize,
) -> Vec<ExtractedReconciliationRow> {
    let variant = metric_variant(&rows[metric_row].cells.join(" "));
    let start = (0..=metric_row)
        .rev()
        .find(|index| {
            let label = rows[*index].cells.first().map(String::as_str).unwrap_or("");
            let lower = label.to_ascii_lowercase();
            lower.contains("net income")
                || lower.contains("net loss")
                || lower.contains("net (loss)")
                || (variant != Some("nareitFfo") && metric_variant(&lower) == Some("nareitFfo"))
        })
        .unwrap_or(metric_row);
    rows[start..=metric_row]
        .iter()
        .filter_map(|row| {
            let label = row.cells.first()?.clone();
            if label.is_empty() {
                return None;
            }
            let value = normalize_number(&combined_numeric_cell(&row.cells, column_index))?;
            Some(ExtractedReconciliationRow { label, value })
        })
        .collect()
}

pub(super) fn normalize_number(raw: &str) -> Option<f64> {
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
    {
        return None;
    }
    let parsed = normalized.parse::<f64>().ok()?;
    Some(if negative { -parsed } else { parsed })
}

fn is_candidate_label(label: &str) -> bool {
    is_weighted_average_diluted_shares(label) || metric_variant(label).is_some()
}

fn is_weighted_average_diluted_shares(label: &str) -> bool {
    let lower = label.to_ascii_lowercase();
    lower.contains("share")
        && lower.contains("diluted")
        && (lower.contains("weighted average") || lower.contains("weighted-average"))
        && contains_ffo_metric(&lower)
}

pub(super) fn contains_ffo_metric(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    lower.contains("funds from operations")
        || lower
            .split(|character: char| !character.is_ascii_alphanumeric())
            .any(|word| matches!(word, "ffo" | "affo" | "nffo"))
}

fn metric_variant(label: &str) -> Option<&'static str> {
    let lower = label.to_ascii_lowercase();
    if !contains_ffo_metric(&lower)
        || [
            "impact",
            "margin",
            "multiple",
            "growth",
            "change in",
            "guidance",
        ]
        .iter()
        .any(|phrase| lower.contains(phrase))
        || (lower.contains("share")
            && (lower.contains("used to calculate") || lower.contains("weighted average")))
    {
        return None;
    }
    if lower.contains("affo") || lower.contains("adjusted funds from operations") {
        Some("affo")
    } else if lower.contains("normalized") || lower.contains("normalised") || lower.contains("nffo")
    {
        Some("normalizedFfo")
    } else if lower.contains("core ffo") || lower.contains("core funds from operations") {
        Some("coreFfo")
    } else if lower.contains("pro forma") || lower.contains("pro-forma") {
        Some("proFormaFfo")
    } else if lower.contains("adjusted ffo") {
        Some("adjustedFfo")
    } else if lower.contains("nareit")
        || lower.contains("funds from operations")
        || lower.contains("ffo")
    {
        Some("nareitFfo")
    } else {
        None
    }
}

pub(super) fn metric_variant_for_reconciliation(label: &str) -> Option<&'static str> {
    metric_variant(label)
}

fn is_per_share_label(label: &str) -> bool {
    let lower = label.to_ascii_lowercase();
    lower.contains("share") && lower.contains("per")
}

pub(super) fn parse_period(text: &str) -> Option<(String, String)> {
    let lower = text.to_ascii_lowercase();
    let period_type = if lower.contains("three months") || lower.contains("quarter ended") {
        "quarter"
    } else if lower.contains("six months") {
        "sixMonths"
    } else if lower.contains("nine months") {
        "nineMonths"
    } else if lower.contains("twelve months") || lower.contains("year ended") {
        "year"
    } else {
        return None;
    };
    let (month, month_number) = [
        ("january", 1),
        ("february", 2),
        ("march", 3),
        ("april", 4),
        ("may", 5),
        ("june", 6),
        ("july", 7),
        ("august", 8),
        ("september", 9),
        ("october", 10),
        ("november", 11),
        ("december", 12),
    ]
    .into_iter()
    .find(|(month, _)| lower.contains(month))?;
    let after_month = lower.split_once(month)?.1;
    let day = after_month
        .split(|character: char| !character.is_ascii_digit())
        .find(|token| !token.is_empty() && token.len() <= 2)?
        .parse::<u8>()
        .ok()?;
    let year = lower
        .split(|character: char| !character.is_ascii_digit())
        .filter(|token| token.len() == 4 && token.starts_with("20"))
        .find_map(|token| token.parse::<u16>().ok())?;
    time::Date::from_calendar_date(year as i32, time::Month::try_from(month_number).ok()?, day)
        .ok()?;
    Some((
        period_type.to_owned(),
        format!("{year:04}-{month_number:02}-{day:02}"),
    ))
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
}

fn detect_units(text: &str) -> Option<String> {
    let lower = text.to_ascii_lowercase();
    let shares_are_unscaled = lower.contains("except share and per share")
        || lower.contains("except shares and per share")
        || lower.contains("except share amounts and per share");
    let thousands = lower
        .rfind("in thousands")
        .or_else(|| lower.rfind("amounts in thousands"));
    let millions = lower
        .rfind("in millions")
        .or_else(|| lower.rfind("amounts in millions"));
    if thousands.is_some() && thousands > millions {
        Some(
            if shares_are_unscaled {
                "thousandsExceptShares"
            } else {
                "thousands"
            }
            .to_owned(),
        )
    } else if millions.is_some() {
        Some(
            if shares_are_unscaled {
                "millionsExceptShares"
            } else {
                "millions"
            }
            .to_owned(),
        )
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source() -> FfoSourceDocument {
        FfoSourceDocument {
            url: "https://www.sec.gov/Archives/earnings.htm".to_owned(),
            exhibit_type: "EX-99.1".to_owned(),
            description: "Earnings release".to_owned(),
            filing_date: "2026-05-07".to_owned(),
            accession_number: "0000123-26-000001".to_owned(),
            filing_form: "8-K".to_owned(),
            filing_index_url: "https://www.sec.gov/Archives/index.html".to_owned(),
        }
    }

    #[test]
    fn parses_complete_periods() {
        assert_eq!(
            parse_period("Three Months Ended March 31, | 2026"),
            Some(("quarter".to_owned(), "2026-03-31".to_owned()))
        );
        assert_eq!(parse_period("Three Months Ended March 31,"), None);
    }

    #[test]
    fn associates_multirow_headers_and_local_units() {
        let html = r#"
            <p>NAREIT FFO and Normalized FFO Reconciliation</p>
            <p>For the Three Months Ended March 31, 2026 and 2025</p>
            <p>(In thousands, except share and per share amounts)</p>
            <table>
              <tr><td></td><td></td><td colspan="6">Three Months Ended March 31,</td><td></td></tr>
              <tr><td></td><td></td><td colspan="2">2026</td><td></td><td></td><td colspan="2">2025</td><td></td></tr>
              <tr><td>Normalized FFO attributable to controlling interest</td><td></td><td>$</td><td>94,815</td><td></td><td></td><td>$</td><td>59,742</td><td></td></tr>
              <tr><td>Normalized FFO per common share attributable to controlling interest — diluted</td><td></td><td>$</td><td>0.50</td><td></td><td></td><td>$</td><td>0.38</td><td></td></tr>
            </table>
        "#;

        let values = extract_values_from_html(html, &source());
        assert_eq!(values.len(), 4);
        assert_eq!(
            values[0].reporting_period,
            "Three Months Ended March 31, | 2026"
        );
        assert_eq!(
            values[1].reporting_period,
            "Three Months Ended March 31, | 2025"
        );
        assert_eq!(values[0].units.as_deref(), Some("thousandsExceptShares"));
        assert_eq!(values[2].value_type, "perShare");
        assert_eq!(values[2].value, 0.50);
    }
}
