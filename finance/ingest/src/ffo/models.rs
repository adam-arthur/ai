use serde::{Deserialize, Serialize};

use crate::common::alpaca_api::CorporateActions;

/// A filing document likely to contain a reported FFO reconciliation.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
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

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReitFfoSources {
    pub cik: String,
    pub documents: Vec<FfoSourceDocument>,
}

/// A period for which an issuer reported actual results.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FfoReportingPeriod {
    /// `quarter`, `sixMonths`, `nineMonths`, or `year`.
    #[serde(rename = "type")]
    pub period_type: String,
    pub end_date: String,
}

/// One issuer-defined FFO measure. Monetary totals are normalized to USD millions while
/// per-share values remain USD per share.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FfoMeasure {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reported_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diluted_per_share: Option<f64>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FfoMeasures {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nareit_ffo: Option<FfoMeasure>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub normalized_ffo: Option<FfoMeasure>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub core_ffo: Option<FfoMeasure>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub adjusted_ffo: Option<FfoMeasure>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pro_forma_ffo: Option<FfoMeasure>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub affo: Option<FfoMeasure>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub weighted_average_diluted_shares: Option<f64>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FfoAdjustment {
    pub name: String,
    pub value: f64,
}

/// Parsed constituent values from the reconciliation. Values are USD millions.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FfoReconciliation {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub net_income: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub real_estate_depreciation_amortization: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub real_estate_impairments: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gain_loss_on_real_estate_sales: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unconsolidated_venture_adjustments: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub noncontrolling_interest_adjustments: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nareit_ffo: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub normalized_ffo: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub core_ffo: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub adjusted_ffo: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pro_forma_ffo: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub affo: Option<f64>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub other_adjustments: Vec<FfoAdjustment>,
}

/// Canonical actual results for one reporting period.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FfoPeriodResult {
    pub period: FfoReportingPeriod,
    pub filed_date: String,
    pub currency: String,
    pub amount_scale: String,
    pub share_scale: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attribution: Option<String>,
    pub measures: FfoMeasures,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reconciliation: Option<FfoReconciliation>,
}

/// Human-readable, deduplicated FFO data persisted for one issuer.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReitFfoData {
    pub symbol: String,
    pub cik: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub name_changes: Vec<FfoNameChange>,
    pub periods: Vec<FfoPeriodResult>,
}

/// A dated issuer symbol change, retained as context for interpreting historical filings.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FfoNameChange {
    pub date: String,
    pub old_symbol: String,
    pub new_symbol: String,
}

pub(crate) fn ffo_name_changes(corporate_actions: &[CorporateActions]) -> Vec<FfoNameChange> {
    corporate_actions
        .iter()
        .flat_map(|actions| {
            actions.name_changes.iter().map(|change| FfoNameChange {
                date: actions.date.clone(),
                old_symbol: change.old_symbol.clone(),
                new_symbol: change.new_symbol.clone(),
            })
        })
        .collect()
}

// The following types are deliberately internal. They retain just enough context to build the
// canonical model above and are never serialized into data/ffo/*.json.

#[derive(Clone, Debug)]
pub(super) struct ExtractedReconciliationRow {
    pub label: String,
    pub value: f64,
}

#[derive(Clone, Debug)]
pub(super) struct ReportedFfoValue {
    pub variant: String,
    pub company_label: String,
    pub value_type: String,
    pub value: f64,
    pub reporting_period: String,
    pub units: Option<String>,
    pub reconciliation: Vec<ExtractedReconciliationRow>,
}

#[derive(Clone, Debug)]
pub(super) struct ExtractedFfoDocument {
    pub document: FfoSourceDocument,
    pub values: Vec<ReportedFfoValue>,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn extracts_dated_name_changes_from_corporate_actions() {
        let corporate_actions = serde_json::from_value::<Vec<CorporateActions>>(json!([
            {
                "date": "2026-03-02",
                "name_changes": [{
                    "old_symbol": "AHH",
                    "new_symbol": "AHRT"
                }]
            },
            {
                "date": "2026-04-02",
                "cash_dividends": [{
                    "rate": 0.14,
                    "ex_date": "2026-03-26"
                }]
            }
        ]))
        .unwrap();

        assert_eq!(
            ffo_name_changes(&corporate_actions),
            vec![FfoNameChange {
                date: "2026-03-02".to_owned(),
                old_symbol: "AHH".to_owned(),
                new_symbol: "AHRT".to_owned(),
            }]
        );
    }

    #[test]
    fn old_ffo_data_without_name_changes_remains_readable() {
        let data = serde_json::from_value::<ReitFfoData>(json!({
            "symbol": "AHRT",
            "cik": "1632970",
            "periods": []
        }))
        .unwrap();

        assert!(data.name_changes.is_empty());
    }
}
