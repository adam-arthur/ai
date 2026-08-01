use std::{collections::HashMap, num::NonZeroU32};

use governor::{
    Quota, RateLimiter, clock::{QuantaClock, QuantaInstant}, middleware::NoOpMiddleware, state::{InMemoryState, NotKeyed}
};
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::Value;

use crate::{
    financials::models::Sector, ingest_utils::common::INGEST_SETTINGS, meta_utils::YieldWatchError
};

use super::HTTP;

static SA_API_RATE_LIMITER: Lazy<
    RateLimiter<NotKeyed, InMemoryState, QuantaClock, NoOpMiddleware<QuantaInstant>>,
> = Lazy::new(|| {
    RateLimiter::direct(
        Quota::with_period(INGEST_SETTINGS.sa_throttle_duration)
            .unwrap()
            .allow_burst(NonZeroU32::new(1).unwrap()),
    )
});

pub async fn fetch_sector_quotes(symbols: &[String]) -> HashMap<String, Sector> {
    if symbols.is_empty() {
        return HashMap::new();
    }

    let url = format!(
        "/symbol_data?fields[]=primarygics&slugs={}",
        symbols.join(",")
    );

    let sectors_response = seeking_alpha::<HashMap<String, Value>>(url.clone())
        .await
        .unwrap();

    sectors_response["data"]
        .as_array()
        .expect("Failed to unwrap 'data'")
        .iter()
        .map(|v| {
            let symbol: String = v["id"]
                .as_str()
                .expect("Failed to parse 'id' (Symbol)")
                .into();
            let sub_industry_gics = v["attributes"]["primarygics"].as_u64();
            log::trace!("{} - {:?} - Processing...", symbol, sub_industry_gics);

            // TODO: Avoid string manipulation
            // let sub_industry_gics_str = sub_industry_gics.to_string();
            // let sub_industry_gics = sub_industry_gics_str.parse::<u32>().unwrap();
            // let sector_gics = sub_industry_gics_str[0..2].parse::<u32>().unwrap();
            // let industry_group_gics = sub_industry_gics_str[0..4].parse::<u32>().unwrap();
            // let industry_gics = sub_industry_gics_str[0..6].parse::<u32>().unwrap();

            (
                symbol.clone(),
                Sector {
                    symbol,
                    sub_industry_gics,
                },
            )
        })
        .collect()
}

#[allow(dead_code)]
pub async fn fetch_quarterly_data(symbols: &[String]) -> HashMap<String, Vec<QuarterlyData>> {
    #[derive(Serialize, Deserialize, Debug)]
    struct SAQuarterlyData {
        // [ticker_id] -> [metric_name] -> values
        estimates: HashMap<String, HashMap<String, Vec<SAQuarterlyValue>>>,
        // revisions: HashMap<String, QuarterlyFundamantalData> Do later
    }
    #[derive(Serialize, Deserialize, Debug)]
    struct SAQuarterlyValue {
        effectivedate: String,
        dataitemvalue: f64,
        period: SAPeriod,
    }

    #[derive(Serialize, Deserialize, Debug)]
    struct SAPeriod {
        periodtypeid: String,
        fiscalquarter: u8,
        fiscalyear: u16,
        calendarquarter: u8,
        calendaryear: u16,
        periodenddate: String,
        advancedate: String,
    }

    if symbols.is_empty() {
        return HashMap::new();
    }

    // Reported revenue and GAAP EPS come from SEC Company Facts. Seeking Alpha remains
    // the source for non-GAAP/REIT metrics and, later, consensus estimates.
    let url = format!(
        "/symbol_data/estimates?period_type=quarterly&estimates_data_items=ffo_actual,eps_normalized&relative_periods=0,-1,-2,-3,-4,-5,-6,-7,-8,-9,-10,-11,-12,-13,-14,-15,-16,-17,-18,-19,-20,-21,-22,-23&ticker_ids={}",
        symbols.join(",")
    );

    let response = seeking_alpha::<SAQuarterlyData>(url.clone()).await.unwrap();

    let mut id_to_tickerquarters: HashMap<String, Vec<QuarterlyData>> = HashMap::new();
    for (ticker_id, data) in response.estimates.into_iter() {
        let mut id_to_quarter: HashMap<String, QuarterlyData> = HashMap::new();

        for (metric_name, values) in data.into_iter() {
            for value in values {
                // Quarter id is defined by end date. Should always be unique right?
                let quarter_id = value.period.periodenddate.clone();
                if !id_to_quarter.contains_key(&quarter_id) {
                    id_to_quarter.insert(
                        // TODO: Change iteration to avoid the need to clone
                        quarter_id.clone(),
                        QuarterlyData {
                            end_date: value.period.periodenddate.clone(),
                            fiscal_year: value.period.fiscalyear,
                            fiscal_quarter: value.period.fiscalquarter,
                            calendar_year: value.period.calendaryear,
                            calendar_quarter: value.period.calendarquarter,
                            ..Default::default()
                        },
                    );
                }
                let quarter = id_to_quarter
                    .get_mut(&quarter_id)
                    .expect("Quarter should exist since added in loop.");

                match metric_name.as_str() {
                    "eps_normalized" => {
                        quarter.earnings_per_share_normalized = Some(value.dataitemvalue)
                    }
                    "ffo_actual" => quarter.ffo_per_share = Some(value.dataitemvalue),
                    _ => panic!("Unknown `metric_name` of {}", metric_name),
                }
            }
        }

        let mut quarters_asc = id_to_quarter.into_values().collect::<Vec<_>>();
        quarters_asc.sort_unstable_by(|a, b| {
            if a.calendar_year != b.calendar_year {
                a.calendar_year.cmp(&b.calendar_year)
            } else {
                a.calendar_quarter.cmp(&b.calendar_quarter)
            }
        });
        id_to_tickerquarters.insert(ticker_id, quarters_asc);
    }

    id_to_tickerquarters
}

async fn seeking_alpha<T>(path: String) -> Result<T, YieldWatchError>
where
    T: DeserializeOwned,
{
    SA_API_RATE_LIMITER.until_ready().await;

    let url = format!("https://seekingalpha.com/api/v3{}", path);
    let response = HTTP.get(
        url
    )
    .header("Referer", "https://seekingalpha.com")
    .header("Origin", "https://seekingalpha.com")
    .header("User-Agent", "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/150.0.0.0 Safari/537.36")
    .send()
    .await?;

    log::debug!("SeekingAlpha API - {} - \"{}\"", response.status(), path);

    let text_body = response.text().await.unwrap();

    // AIf you get this, solve the captcha in a browser
    if text_body.contains("blockScript") {
        panic!("SeekingAlpha - captcha");
    }

    Ok(serde_json::from_str::<T>(&text_body)
        .unwrap_or_else(|error| panic!("Failed to deserialize JSON: {} \n {}", text_body, error)))
}

pub use crate::financials::models::QuarterlyData;
