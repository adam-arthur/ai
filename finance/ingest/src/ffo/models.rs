use serde::Serialize;

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
