use std::{collections::HashMap, env, num::NonZeroU32, time::Duration};

use governor::{DefaultDirectRateLimiter, Quota, RateLimiter};
use once_cell::sync::Lazy;
use serde::{Deserialize, de::DeserializeOwned};

use crate::{financials::models::Company, meta_utils::YieldWatchError};

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
}
