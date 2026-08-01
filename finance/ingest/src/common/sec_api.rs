use std::{
    collections::{BTreeMap, HashMap}, env, num::NonZeroU32, time::Duration
};

use governor::{DefaultDirectRateLimiter, Quota, RateLimiter};
use once_cell::sync::Lazy;
use serde::{Deserialize, de::DeserializeOwned};
use time::{Date, macros::format_description};

use crate::{
    financials::models::{Company, QuarterlyData}, meta_utils::YieldWatchError
};

use super::HTTP;

static SEC_API_RATE_LIMITER: Lazy<DefaultDirectRateLimiter> = Lazy::new(|| {
    // Stay below the SEC fair-access ceiling of ten requests per second.
    RateLimiter::direct(
        Quota::with_period(Duration::from_millis(125))
            .expect("SEC request period must be non-zero")
            .allow_burst(NonZeroU32::new(1).unwrap()),
    )
});
static SEC_USER_AGENT: Lazy<String> = Lazy::new(|| {
    env::var("SEC_USER_AGENT")
        .expect("SEC_USER_AGENT must identify the application and include contact information")
});

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SecCompanySubmission {
    name: String,
    #[serde(default)]
    tickers: Vec<String>,
    #[serde(default)]
    exchanges: Vec<String>,
    sic: Option<String>,
    sic_description: Option<String>,
    description: Option<String>,
    website: Option<String>,
    investor_website: Option<String>,
    addresses: Option<SecAddresses>,
    phone: Option<String>,
    state_of_incorporation: Option<String>,
    fiscal_year_end: Option<String>,
    #[serde(default)]
    filings: SecFilings,
}

#[derive(Default, Deserialize)]
struct SecFilings {
    #[serde(default)]
    recent: SecRecentFilings,
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SecRecentFilings {
    #[serde(default)]
    accession_number: Vec<String>,
    #[serde(default)]
    filing_date: Vec<String>,
    #[serde(default)]
    report_date: Vec<String>,
    #[serde(default)]
    acceptance_date_time: Vec<String>,
    #[serde(default)]
    form: Vec<String>,
    #[serde(default)]
    items: Vec<String>,
    #[serde(default)]
    primary_document: Vec<String>,
    #[serde(default)]
    primary_doc_description: Vec<String>,
}

/// An annual, quarterly, or current report listed in a company's recent SEC submissions.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SecFiling {
    pub accession_number: String,
    pub filing_date: String,
    pub report_date: Option<String>,
    pub acceptance_date_time: Option<String>,
    pub form: String,
    pub items: Vec<String>,
    pub primary_document: String,
    pub primary_document_description: Option<String>,
    pub filing_index_url: String,
    pub primary_document_url: String,
}

#[derive(Deserialize)]
struct SecAddresses {
    business: Option<SecAddress>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SecAddress {
    street1: Option<String>,
    street2: Option<String>,
    city: Option<String>,
    state_or_country: Option<String>,
    zip_code: Option<String>,
    country: Option<String>,
}

#[derive(Deserialize)]
struct SecCompanyFacts {
    #[serde(default)]
    facts: HashMap<String, HashMap<String, SecFact>>,
}

#[derive(Deserialize)]
struct SecFact {
    #[serde(default)]
    units: HashMap<String, Vec<SecFactValue>>,
}

#[derive(Clone, Deserialize)]
struct SecFactValue {
    start: Option<String>,
    end: String,
    val: f64,
    fy: Option<u16>,
    fp: Option<String>,
    form: String,
    filed: String,
}

const REVENUE_CONCEPTS: &[&str] = &[
    "RevenueFromContractWithCustomerExcludingAssessedTax",
    "RevenueFromContractWithCustomerIncludingAssessedTax",
    "Revenues",
    "SalesRevenueNet",
];
const EPS_CONCEPTS: &[&str] = &["EarningsPerShareDiluted", "EarningsPerShareBasic"];

pub async fn fetch_symbol_to_cik() -> HashMap<String, u64> {
    #[derive(Deserialize, Debug)]
    struct SecSymbol {
        /// Numerical cik
        cik_str: u64,

        /// AAPL
        ticker: String,

        /// Apple Inc.
        #[allow(dead_code)]
        title: String,
    }

    let result = sec::<HashMap<u32, SecSymbol>>("/files/company_tickers.json".into()).await;

    result
        .unwrap()
        .values()
        .map(|v| (v.ticker.clone(), v.cik_str))
        .collect()
}

pub async fn fetch_company(symbol: &str, cik: &str) -> Result<Company, YieldWatchError> {
    let padded_cik = format!("{:0>10}", cik.trim_start_matches('0'));
    let submission =
        sec_data::<SecCompanySubmission>(format!("/submissions/CIK{padded_cik}.json")).await?;

    Ok(submission.into_company(symbol))
}

/// Fetches the 8-K, 10-Q, and 10-K filings in the SEC's recent-submissions index.
///
/// Amendments to those forms are included. The SEC keeps older submissions in separate files;
/// this function intentionally returns only the `filings.recent` portion of the company response.
#[allow(dead_code)]
pub async fn fetch_recent_filings(cik: &str) -> Result<Vec<SecFiling>, YieldWatchError> {
    let unpadded_cik = cik.trim_start_matches('0');
    let padded_cik = format!("{:0>10}", unpadded_cik);
    let submission =
        sec_data::<SecCompanySubmission>(format!("/submissions/CIK{padded_cik}.json")).await?;

    Ok(submission.filings.recent.into_filings(unpadded_cik))
}

/// Fetches reported, quarter-only revenue and EPS from the SEC Company Facts API.
///
/// Company Facts also contains cumulative year-to-date observations. Those are excluded by
/// selecting 10-Q facts whose duration is consistent with a single fiscal quarter. Fiscal Q4 EPS
/// is intentionally not synthesized from annual EPS because the weighted-average share denominator
/// makes subtraction invalid.
#[allow(dead_code)]
pub async fn fetch_reported_quarterly_data(
    cik: &str,
) -> Result<Vec<QuarterlyData>, YieldWatchError> {
    let padded_cik = format!("{:0>10}", cik.trim_start_matches('0'));
    let facts =
        sec_data::<SecCompanyFacts>(format!("/api/xbrl/companyfacts/CIK{padded_cik}.json")).await?;

    Ok(facts.into_reported_quarters())
}

impl SecCompanyFacts {
    fn into_reported_quarters(self) -> Vec<QuarterlyData> {
        let Some(us_gaap) = self.facts.get("us-gaap") else {
            return Vec::new();
        };
        let mut quarters = BTreeMap::<(String, u16, u8), QuarterlyData>::new();

        merge_concepts(
            &mut quarters,
            us_gaap,
            REVENUE_CONCEPTS,
            "USD",
            |quarter, value| quarter.revenue = Some(value),
        );
        merge_concepts(
            &mut quarters,
            us_gaap,
            EPS_CONCEPTS,
            "USD/shares",
            |quarter, value| quarter.earnings_per_share = Some(value),
        );

        quarters.into_values().collect()
    }
}

impl SecRecentFilings {
    fn into_filings(self, cik: &str) -> Vec<SecFiling> {
        let Self {
            accession_number,
            filing_date,
            report_date,
            acceptance_date_time,
            form,
            items,
            primary_document,
            primary_doc_description,
        } = self;

        form.into_iter()
            .enumerate()
            .filter(|(_, form)| is_reit_source_form(form))
            .filter_map(|(index, form)| {
                let accession_number = accession_number.get(index)?.clone();
                let filing_date = filing_date.get(index)?.clone();
                let primary_document = primary_document.get(index)?.clone();
                let accession_path = accession_number.replace('-', "");
                let archive_base =
                    format!("https://www.sec.gov/Archives/edgar/data/{cik}/{accession_path}");

                Some(SecFiling {
                    filing_index_url: format!("{archive_base}/{accession_number}-index.html"),
                    primary_document_url: format!("{archive_base}/{primary_document}"),
                    accession_number,
                    filing_date,
                    report_date: report_date.get(index).cloned().and_then(non_empty),
                    acceptance_date_time: acceptance_date_time
                        .get(index)
                        .cloned()
                        .and_then(non_empty),
                    form,
                    items: items
                        .get(index)
                        .map(|items| {
                            items
                                .split(',')
                                .map(str::trim)
                                .filter(|item| !item.is_empty())
                                .map(str::to_owned)
                                .collect()
                        })
                        .unwrap_or_default(),
                    primary_document,
                    primary_document_description: primary_doc_description
                        .get(index)
                        .cloned()
                        .and_then(non_empty),
                })
            })
            .collect()
    }
}

fn is_reit_source_form(form: &str) -> bool {
    matches!(
        form.strip_suffix("/A").unwrap_or(form),
        "8-K" | "10-Q" | "10-K"
    )
}

fn merge_concepts(
    quarters: &mut BTreeMap<(String, u16, u8), QuarterlyData>,
    taxonomy: &HashMap<String, SecFact>,
    concepts: &[&str],
    unit: &str,
    set_value: impl Fn(&mut QuarterlyData, f64) + Copy,
) {
    // Concepts are ordered from preferred to fallback. A fallback only fills periods that the
    // preferred concept did not cover, which also handles issuers changing tags over time.
    for concept in concepts {
        let Some(values) = taxonomy.get(*concept).and_then(|fact| fact.units.get(unit)) else {
            continue;
        };

        for value in current_quarter_facts(values) {
            let fiscal_year = value.fy.expect("validated SEC fact has a fiscal year");
            let fiscal_quarter =
                fiscal_quarter(&value.fp).expect("validated SEC fact has a fiscal quarter");
            let key = (value.end.clone(), fiscal_year, fiscal_quarter);
            let quarter = quarters.entry(key).or_insert_with(|| {
                let end = parse_sec_date(&value.end).expect("validated SEC fact has a valid end");
                QuarterlyData {
                    end_date: value.end.clone(),
                    start_date: value.start.clone(),
                    fiscal_year,
                    fiscal_quarter,
                    calendar_year: end.year() as u16,
                    calendar_quarter: (u8::from(end.month()) - 1) / 3 + 1,
                    ..Default::default()
                }
            });

            let already_set = if unit == "USD" {
                quarter.revenue.is_some()
            } else {
                quarter.earnings_per_share.is_some()
            };
            if !already_set {
                set_value(quarter, value.val);
            }
        }
    }
}

fn current_quarter_facts(values: &[SecFactValue]) -> Vec<&SecFactValue> {
    let mut by_period = BTreeMap::<(String, u16, u8), &SecFactValue>::new();

    for value in values.iter().filter(|value| is_current_quarter_fact(value)) {
        let key = (
            value.end.clone(),
            value.fy.expect("validated SEC fact has a fiscal year"),
            fiscal_quarter(&value.fp).expect("validated SEC fact has a fiscal quarter"),
        );
        let replace = by_period
            .get(&key)
            .is_none_or(|existing| value.filed > existing.filed);
        if replace {
            by_period.insert(key, value);
        }
    }

    by_period.into_values().collect()
}

fn is_current_quarter_fact(value: &SecFactValue) -> bool {
    if !matches!(value.form.as_str(), "10-Q" | "10-Q/A")
        || value.fy.is_none()
        || fiscal_quarter(&value.fp).is_none()
    {
        return false;
    }

    let (Some(start), Some(end), Some(filed)) = (
        value.start.as_deref().and_then(parse_sec_date),
        parse_sec_date(&value.end),
        parse_sec_date(&value.filed),
    ) else {
        return false;
    };
    let duration_days = (end - start).whole_days();
    let filing_delay_days = (filed - end).whole_days();

    (45..=150).contains(&duration_days) && (0..=180).contains(&filing_delay_days)
}

fn fiscal_quarter(fp: &Option<String>) -> Option<u8> {
    match fp.as_deref() {
        Some("Q1") => Some(1),
        Some("Q2") => Some(2),
        Some("Q3") => Some(3),
        _ => None,
    }
}

fn parse_sec_date(value: &str) -> Option<Date> {
    Date::parse(value, format_description!("[year]-[month]-[day]")).ok()
}

impl SecCompanySubmission {
    fn into_company(self, symbol: &str) -> Company {
        let exchange = self
            .tickers
            .iter()
            .position(|ticker| ticker.eq_ignore_ascii_case(symbol))
            .and_then(|index| self.exchanges.get(index))
            .cloned()
            .and_then(non_empty);
        let address = self.addresses.and_then(|addresses| addresses.business);

        Company {
            symbol: symbol.to_owned(),
            company_name: self.name,
            exchange,
            industry: self.sic_description.and_then(non_empty),
            website: self.website.and_then(non_empty),
            investor_website: self.investor_website.and_then(non_empty),
            description: self.description.and_then(non_empty),
            primary_sic_code: self.sic.and_then(|sic| sic.parse().ok()),
            address: address
                .as_ref()
                .and_then(|address| address.street1.clone())
                .and_then(non_empty),
            address2: address
                .as_ref()
                .and_then(|address| address.street2.clone())
                .and_then(non_empty),
            state: address
                .as_ref()
                .and_then(|address| address.state_or_country.clone())
                .and_then(non_empty),
            city: address
                .as_ref()
                .and_then(|address| address.city.clone())
                .and_then(non_empty),
            zip: address
                .as_ref()
                .and_then(|address| address.zip_code.clone())
                .and_then(non_empty),
            country: address
                .and_then(|address| address.country)
                .and_then(non_empty),
            phone: self.phone.and_then(non_empty),
            state_of_incorporation: self.state_of_incorporation.and_then(non_empty),
            fiscal_year_end: self.fiscal_year_end.and_then(non_empty),
        }
    }
}

fn non_empty(value: String) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_owned())
}

async fn sec<T>(path: String) -> Result<T, YieldWatchError>
where
    T: DeserializeOwned,
{
    fetch_sec_json("https://www.sec.gov", path).await
}

async fn sec_data<T>(path: String) -> Result<T, YieldWatchError>
where
    T: DeserializeOwned,
{
    fetch_sec_json("https://data.sec.gov", path).await
}

async fn fetch_sec_json<T>(base_url: &str, path: String) -> Result<T, YieldWatchError>
where
    T: DeserializeOwned,
{
    if !path.starts_with('/') {
        panic!("Invalid path!")
    }

    SEC_API_RATE_LIMITER.until_ready().await;

    let response = HTTP
        .get(format!("{base_url}{path}"))
        .header("User-Agent", SEC_USER_AGENT.as_str())
        .send()
        .await?
        .error_for_status()?;

    Ok(response.json::<T>().await?)
}

/// Fetches a text document from SEC.gov using the shared fair-access throttle and user agent.
pub(super) async fn fetch_sec_text(url: &str) -> Result<String, YieldWatchError> {
    SEC_API_RATE_LIMITER.until_ready().await;

    Ok(HTTP
        .get(url)
        .header("User-Agent", SEC_USER_AGENT.as_str())
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn sec_submission_maps_company_and_discards_blank_fields() {
        let submission = serde_json::from_value::<SecCompanySubmission>(json!({
            "name": "Apple Inc.",
            "tickers": ["AAPL", "APC.F"],
            "exchanges": ["Nasdaq", "Frankfurt"],
            "sic": "3571",
            "sicDescription": "Electronic Computers",
            "description": " Apple makes consumer technology. ",
            "website": " https://www.apple.com ",
            "investorWebsite": "",
            "phone": "(408) 996-1010",
            "stateOfIncorporation": "CA",
            "fiscalYearEnd": "0927",
            "addresses": {
                "business": {
                    "street1": "ONE APPLE PARK WAY",
                    "street2": "",
                    "city": "CUPERTINO",
                    "stateOrCountry": "CA",
                    "zipCode": "95014",
                    "country": null
                }
            }
        }))
        .unwrap();

        assert_eq!(
            serde_json::to_value(submission.into_company("AAPL")).unwrap(),
            json!({
                "symbol": "AAPL",
                "companyName": "Apple Inc.",
                "exchange": "Nasdaq",
                "industry": "Electronic Computers",
                "website": "https://www.apple.com",
                "description": "Apple makes consumer technology.",
                "primarySicCode": 3571,
                "address": "ONE APPLE PARK WAY",
                "state": "CA",
                "city": "CUPERTINO",
                "zip": "95014",
                "phone": "(408) 996-1010",
                "stateOfIncorporation": "CA",
                "fiscalYearEnd": "0927"
            })
        );
    }

    #[test]
    fn sec_submission_maps_recent_reit_source_filings_and_urls() {
        let submission = serde_json::from_value::<SecCompanySubmission>(json!({
            "name": "Example REIT",
            "filings": {
                "recent": {
                    "accessionNumber": [
                        "0001234567-26-000010",
                        "0001234567-26-000009",
                        "0001234567-26-000008",
                        "0001234567-26-000007"
                    ],
                    "filingDate": ["2026-05-01", "2026-04-30", "2026-04-29", "2026-04-28"],
                    "reportDate": ["2026-03-31", "2026-03-31", "2025-12-31", ""],
                    "acceptanceDateTime": [
                        "2026-05-01T16:01:02.000Z",
                        "2026-04-30T16:01:02.000Z",
                        "2026-04-29T16:01:02.000Z",
                        "2026-04-28T16:01:02.000Z"
                    ],
                    "form": ["8-K", "10-Q", "10-K/A", "4"],
                    "items": ["2.02,9.01", "", "", ""],
                    "primaryDocument": ["earnings.htm", "quarter.htm", "annual.htm", "form4.xml"],
                    "primaryDocDescription": ["Earnings release", "10-Q", "", "FORM 4"]
                }
            }
        }))
        .unwrap();

        let filings = submission.filings.recent.into_filings("1234567");

        assert_eq!(filings.len(), 3);
        assert_eq!(filings[0].form, "8-K");
        assert_eq!(filings[0].items, ["2.02", "9.01"]);
        assert_eq!(filings[0].report_date.as_deref(), Some("2026-03-31"));
        assert_eq!(
            filings[0].filing_index_url,
            "https://www.sec.gov/Archives/edgar/data/1234567/000123456726000010/0001234567-26-000010-index.html"
        );
        assert_eq!(
            filings[0].primary_document_url,
            "https://www.sec.gov/Archives/edgar/data/1234567/000123456726000010/earnings.htm"
        );
        assert_eq!(filings[2].form, "10-K/A");
        assert_eq!(filings[2].primary_document_description, None);
    }

    #[test]
    fn company_facts_maps_only_reported_single_quarter_revenue_and_eps() {
        let facts = serde_json::from_value::<SecCompanyFacts>(json!({
            "facts": {
                "us-gaap": {
                    "RevenueFromContractWithCustomerExcludingAssessedTax": {
                        "units": { "USD": [
                            {
                                "start": "2025-01-01", "end": "2025-03-31", "val": 100.0,
                                "fy": 2025, "fp": "Q1", "form": "10-Q", "filed": "2025-05-01"
                            },
                            {
                                "start": "2025-01-01", "end": "2025-06-30", "val": 230.0,
                                "fy": 2025, "fp": "Q2", "form": "10-Q", "filed": "2025-08-01"
                            },
                            {
                                "start": "2025-04-01", "end": "2025-06-30", "val": 125.0,
                                "fy": 2025, "fp": "Q2", "form": "10-Q", "filed": "2025-08-01"
                            },
                            {
                                "start": "2025-04-01", "end": "2025-06-30", "val": 130.0,
                                "fy": 2025, "fp": "Q2", "form": "10-Q/A", "filed": "2025-08-15"
                            },
                            {
                                "start": "2025-01-01", "end": "2025-12-31", "val": 500.0,
                                "fy": 2025, "fp": "FY", "form": "10-K", "filed": "2026-02-01"
                            }
                        ]}
                    },
                    "Revenues": {
                        "units": { "USD": [{
                            "start": "2025-07-01", "end": "2025-09-30", "val": 140.0,
                            "fy": 2025, "fp": "Q3", "form": "10-Q", "filed": "2025-11-01"
                        }]}
                    },
                    "EarningsPerShareDiluted": {
                        "units": { "USD/shares": [{
                            "start": "2025-01-01", "end": "2025-03-31", "val": 1.25,
                            "fy": 2025, "fp": "Q1", "form": "10-Q", "filed": "2025-05-01"
                        }]}
                    },
                    "EarningsPerShareBasic": {
                        "units": { "USD/shares": [
                            {
                                "start": "2025-01-01", "end": "2025-03-31", "val": 1.30,
                                "fy": 2025, "fp": "Q1", "form": "10-Q", "filed": "2025-05-01"
                            },
                            {
                                "start": "2025-04-01", "end": "2025-06-30", "val": 1.40,
                                "fy": 2025, "fp": "Q2", "form": "10-Q", "filed": "2025-08-01"
                            }
                        ]}
                    }
                },
                "dei": {}
            }
        }))
        .unwrap();

        let quarters = facts.into_reported_quarters();

        assert_eq!(quarters.len(), 3);
        assert_eq!(quarters[0].start_date.as_deref(), Some("2025-01-01"));
        assert_eq!(quarters[0].revenue, Some(100.0));
        assert_eq!(quarters[0].earnings_per_share, Some(1.25));
        assert_eq!(quarters[0].calendar_quarter, 1);
        assert_eq!(quarters[1].revenue, Some(130.0));
        assert_eq!(quarters[1].earnings_per_share, Some(1.40));
        assert_eq!(quarters[2].revenue, Some(140.0));
        assert_eq!(quarters[2].earnings_per_share, None);
    }

    #[test]
    fn company_facts_ignores_late_comparative_periods() {
        let value = SecFactValue {
            start: Some("2024-01-01".into()),
            end: "2024-03-31".into(),
            val: 10.0,
            fy: Some(2025),
            fp: Some("Q1".into()),
            form: "10-Q".into(),
            filed: "2025-05-01".into(),
        };

        assert!(!is_current_quarter_fact(&value));
    }
}
