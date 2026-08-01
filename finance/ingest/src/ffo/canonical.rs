use std::collections::BTreeMap;

use super::extract::parse_period;
use super::models::ExtractedReconciliationRow;
use super::{
    ExtractedFfoDocument, FfoAdjustment, FfoMeasure, FfoMeasures, FfoPeriodResult, FfoReconciliation, FfoReportingPeriod, ReitFfoData
};

pub(super) fn canonicalize(
    symbol: &str,
    cik: &str,
    mut documents: Vec<ExtractedFfoDocument>,
) -> ReitFfoData {
    documents.sort_by(|left, right| {
        right
            .document
            .filing_date
            .cmp(&left.document.filing_date)
            .then_with(|| source_priority(left).cmp(&source_priority(right)))
    });

    let mut periods = BTreeMap::<(String, String), PeriodAccumulator>::new();
    for document in documents {
        for value in document.values {
            let Some((period_type, end_date)) = parse_period(&value.reporting_period) else {
                continue;
            };
            let period = periods
                .entry((end_date.clone(), period_type.clone()))
                .or_insert_with(|| PeriodAccumulator::new(period_type, end_date));
            period
                .filed_date
                .get_or_insert_with(|| document.document.filing_date.clone());
            if period.attribution.is_none() {
                period.attribution = attribution(&value.company_label).map(str::to_owned);
            }

            if value.value_type == "shares" {
                if period.measures.weighted_average_diluted_shares.is_none() {
                    period.measures.weighted_average_diluted_shares =
                        normalize_shares(value.value, value.units.as_deref());
                }
                continue;
            }

            let Some(measure) = measure_mut(&mut period.measures, &value.variant) else {
                continue;
            };
            if value.value_type == "perShare" {
                if measure.diluted_per_share.is_none() {
                    measure.diluted_per_share = Some(value.value);
                    measure.reported_name.get_or_insert(value.company_label);
                }
            } else if measure.total.is_none()
                && let Some(total) = normalize_amount(value.value, value.units.as_deref())
            {
                measure.total = Some(total);
                measure.reported_name = Some(value.company_label);
                if let Some(reconciliation) =
                    build_reconciliation(&value.reconciliation, value.units.as_deref())
                {
                    merge_reconciliation(&mut period.reconciliation, reconciliation);
                }
            }
        }
    }

    let mut periods = periods
        .into_values()
        .filter(|period| has_measure(&period.measures))
        .map(PeriodAccumulator::finish)
        .collect::<Vec<_>>();
    periods.sort_by(|left, right| {
        right
            .period
            .end_date
            .cmp(&left.period.end_date)
            .then_with(|| {
                period_order(&left.period.period_type).cmp(&period_order(&right.period.period_type))
            })
    });

    ReitFfoData {
        symbol: symbol.to_owned(),
        cik: cik.to_owned(),
        periods,
    }
}

fn source_priority(document: &ExtractedFfoDocument) -> u8 {
    let text = format!(
        "{} {}",
        document.document.description.to_ascii_lowercase(),
        document.document.url.to_ascii_lowercase()
    );
    if text.contains("supplement") {
        0
    } else if text.contains("earning") || text.contains("result") {
        1
    } else if matches!(document.document.filing_form.as_str(), "10-Q" | "10-K") {
        2
    } else {
        3
    }
}

struct PeriodAccumulator {
    period_type: String,
    end_date: String,
    filed_date: Option<String>,
    attribution: Option<String>,
    measures: FfoMeasures,
    reconciliation: Option<FfoReconciliation>,
}

impl PeriodAccumulator {
    fn new(period_type: String, end_date: String) -> Self {
        Self {
            period_type,
            end_date,
            filed_date: None,
            attribution: None,
            measures: FfoMeasures::default(),
            reconciliation: None,
        }
    }

    fn finish(self) -> FfoPeriodResult {
        FfoPeriodResult {
            period: FfoReportingPeriod {
                period_type: self.period_type,
                end_date: self.end_date,
            },
            filed_date: self.filed_date.unwrap_or_default(),
            currency: "USD".to_owned(),
            amount_scale: "millions".to_owned(),
            share_scale: "millions".to_owned(),
            attribution: self.attribution,
            measures: self.measures,
            reconciliation: self.reconciliation,
        }
    }
}

fn measure_mut<'a>(measures: &'a mut FfoMeasures, variant: &str) -> Option<&'a mut FfoMeasure> {
    let slot = match variant {
        "nareitFfo" => &mut measures.nareit_ffo,
        "normalizedFfo" => &mut measures.normalized_ffo,
        "coreFfo" => &mut measures.core_ffo,
        "adjustedFfo" => &mut measures.adjusted_ffo,
        "proFormaFfo" => &mut measures.pro_forma_ffo,
        "affo" => &mut measures.affo,
        _ => return None,
    };
    Some(slot.get_or_insert_with(FfoMeasure::default))
}

fn has_measure(measures: &FfoMeasures) -> bool {
    [
        measures.nareit_ffo.as_ref(),
        measures.normalized_ffo.as_ref(),
        measures.core_ffo.as_ref(),
        measures.adjusted_ffo.as_ref(),
        measures.pro_forma_ffo.as_ref(),
        measures.affo.as_ref(),
    ]
    .into_iter()
    .flatten()
    .any(|measure| measure.total.is_some() || measure.diluted_per_share.is_some())
}

fn normalize_amount(value: f64, units: Option<&str>) -> Option<f64> {
    match units? {
        units if units.starts_with("thousands") => Some(value / 1_000.0),
        units if units.starts_with("millions") => Some(value),
        _ => None,
    }
}

fn normalize_shares(value: f64, units: Option<&str>) -> Option<f64> {
    match units {
        Some(units) if units.ends_with("ExceptShares") => Some(value / 1_000_000.0),
        Some(units) if units.starts_with("thousands") => Some(value / 1_000.0),
        Some(units) if units.starts_with("millions") => Some(value),
        _ if value >= 1_000_000.0 => Some(value / 1_000_000.0),
        _ => None,
    }
}

fn attribution(label: &str) -> Option<&'static str> {
    let lower = label.to_ascii_lowercase();
    if lower.contains("controlling interest") {
        Some("controllingInterest")
    } else if lower.contains("common stockholder") || lower.contains("common shareholder") {
        Some("commonStockholders")
    } else if lower.contains("common stock and units") || lower.contains("common shares and units")
    {
        Some("commonStockAndUnits")
    } else {
        None
    }
}

fn build_reconciliation(
    rows: &[ExtractedReconciliationRow],
    units: Option<&str>,
) -> Option<FfoReconciliation> {
    let mut reconciliation = FfoReconciliation::default();
    let mut other = BTreeMap::<String, f64>::new();
    for row in rows {
        let Some(value) = normalize_amount(row.value, units) else {
            continue;
        };
        let lower = row.label.to_ascii_lowercase();
        if lower.contains("noncontrolling") || lower.contains("non-controlling") {
            add(
                &mut reconciliation.noncontrolling_interest_adjustments,
                value,
            );
        } else if (lower.contains("net income")
            || lower.contains("net loss")
            || lower.contains("net (loss)"))
            && !lower.contains("per share")
        {
            reconciliation.net_income = Some(value);
        } else if lower.contains("depreciation") && lower.contains("real estate") {
            add(
                &mut reconciliation.real_estate_depreciation_amortization,
                value,
            );
        } else if lower.contains("impairment") && lower.contains("real estate") {
            add(&mut reconciliation.real_estate_impairments, value);
        } else if (lower.contains("gain") || lower.contains("loss"))
            && (lower.contains("sale") || lower.contains("disposition"))
            && (lower.contains("real estate") || lower.contains("property"))
        {
            add(&mut reconciliation.gain_loss_on_real_estate_sales, value);
        } else if lower.contains("unconsolidated") || lower.contains("joint venture") {
            add(
                &mut reconciliation.unconsolidated_venture_adjustments,
                value,
            );
        } else if let Some(variant) = super::extract::metric_variant_for_reconciliation(&lower) {
            match variant {
                "nareitFfo" => reconciliation.nareit_ffo = Some(value),
                "normalizedFfo" => reconciliation.normalized_ffo = Some(value),
                "coreFfo" => reconciliation.core_ffo = Some(value),
                "adjustedFfo" => reconciliation.adjusted_ffo = Some(value),
                "proFormaFfo" => reconciliation.pro_forma_ffo = Some(value),
                "affo" => reconciliation.affo = Some(value),
                _ => {}
            }
        } else {
            *other.entry(row.label.clone()).or_default() += value;
        }
    }
    reconciliation.other_adjustments = other
        .into_iter()
        .map(|(name, value)| FfoAdjustment { name, value })
        .collect();
    has_reconciliation(&reconciliation).then_some(reconciliation)
}

fn add(slot: &mut Option<f64>, value: f64) {
    *slot = Some(slot.unwrap_or(0.0) + value);
}

fn has_reconciliation(value: &FfoReconciliation) -> bool {
    value.net_income.is_some()
        || value.real_estate_depreciation_amortization.is_some()
        || value.real_estate_impairments.is_some()
        || value.gain_loss_on_real_estate_sales.is_some()
        || value.unconsolidated_venture_adjustments.is_some()
        || value.noncontrolling_interest_adjustments.is_some()
        || value.nareit_ffo.is_some()
        || value.normalized_ffo.is_some()
        || value.core_ffo.is_some()
        || value.adjusted_ffo.is_some()
        || value.pro_forma_ffo.is_some()
        || value.affo.is_some()
        || !value.other_adjustments.is_empty()
}

fn merge_reconciliation(target: &mut Option<FfoReconciliation>, incoming: FfoReconciliation) {
    let target = target.get_or_insert_with(FfoReconciliation::default);
    macro_rules! fill {
        ($field:ident) => {
            if target.$field.is_none() {
                target.$field = incoming.$field;
            }
        };
    }
    fill!(net_income);
    fill!(real_estate_depreciation_amortization);
    fill!(real_estate_impairments);
    fill!(gain_loss_on_real_estate_sales);
    fill!(unconsolidated_venture_adjustments);
    fill!(noncontrolling_interest_adjustments);
    fill!(nareit_ffo);
    fill!(normalized_ffo);
    fill!(core_ffo);
    fill!(adjusted_ffo);
    fill!(pro_forma_ffo);
    fill!(affo);
    for adjustment in incoming.other_adjustments {
        if !target
            .other_adjustments
            .iter()
            .any(|existing| existing.name == adjustment.name)
        {
            target.other_adjustments.push(adjustment);
        }
    }
    target
        .other_adjustments
        .sort_by(|left, right| left.name.cmp(&right.name));
}

fn period_order(period_type: &str) -> u8 {
    match period_type {
        "quarter" => 0,
        "sixMonths" => 1,
        "nineMonths" => 2,
        "year" => 3,
        _ => 4,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ffo::{FfoSourceDocument, extract::extract_values_from_html};

    #[test]
    fn emits_numeric_period_oriented_data_without_extraction_artifacts() {
        let source = FfoSourceDocument {
            url: "https://www.sec.gov/Archives/earnings.htm".to_owned(),
            exhibit_type: "EX-99.1".to_owned(),
            description: "Earnings release".to_owned(),
            filing_date: "2026-05-07".to_owned(),
            accession_number: "0000123-26-000001".to_owned(),
            filing_form: "8-K".to_owned(),
            filing_index_url: "https://www.sec.gov/Archives/index.html".to_owned(),
        };
        let html = r#"
            <p>NAREIT FFO and Normalized FFO Reconciliation</p>
            <p>(In thousands, except share and per share amounts)</p>
            <table>
              <tr><th></th><th>Three Months Ended March 31, 2026</th></tr>
              <tr><td>Net income attributable to controlling interest</td><td>24,011</td></tr>
              <tr><td>Real estate depreciation and amortization</td><td>66,993</td></tr>
              <tr><td>NAREIT FFO attributable to controlling interest</td><td>90,354</td></tr>
              <tr><td>Normalized FFO attributable to controlling interest</td><td>94,815</td></tr>
              <tr><td>Normalized FFO per common share attributable to controlling interest — diluted</td><td>0.50</td></tr>
            </table>
        "#;
        let values = extract_values_from_html(html, &source);
        let data = canonicalize(
            "AHR",
            "1632970",
            vec![ExtractedFfoDocument {
                document: source,
                values,
            }],
        );

        assert_eq!(data.periods.len(), 1);
        let period = &data.periods[0];
        assert_eq!(period.period.end_date, "2026-03-31");
        assert_eq!(
            period.measures.nareit_ffo.as_ref().unwrap().total,
            Some(90.354)
        );
        assert_eq!(
            period
                .measures
                .normalized_ffo
                .as_ref()
                .unwrap()
                .diluted_per_share,
            Some(0.50)
        );
        let reconciliation = period.reconciliation.as_ref().unwrap();
        assert_eq!(reconciliation.net_income, Some(24.011));
        assert_eq!(
            reconciliation.real_estate_depreciation_amortization,
            Some(66.993)
        );
        assert_eq!(reconciliation.nareit_ffo, Some(90.354));
        assert_eq!(reconciliation.normalized_ffo, Some(94.815));
        let json = serde_json::to_value(&data).unwrap();
        assert!(
            json.pointer("/periods/0/measures/normalizedFfo/total")
                .unwrap()
                .is_number()
        );
        let serialized = serde_json::to_string(&data).unwrap();
        assert!(!serialized.contains("definitions"));
        assert!(!serialized.contains("cells"));
        assert!(!serialized.contains("filingIndexUrl"));
    }
}
