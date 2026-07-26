use std::env;

use serde::Deserialize;
use serde::de::DeserializeOwned;

use crate::{
    financials::models::TreasuryDuration, financials::models::TreasuryRate, meta_utils::YieldWatchError
};

use super::HTTP;

pub async fn fetch_treasury_rates(duration: TreasuryDuration) -> Vec<TreasuryRate> {
    #[derive(Deserialize, Debug)]
    struct RawTreasuryRates {
        observations: Vec<RawTreasuryRate>,
    }

    #[derive(Deserialize, Debug)]
    struct RawTreasuryRate {
        date: String,
        value: String,
    }

    let result = fred::<RawTreasuryRates>(format!(
        "/series/observations?series_id={}",
        duration.as_series_name(),
    ))
    .await;

    result
        .unwrap()
        .observations
        .into_iter()
        .map(|v| TreasuryRate {
            date: v.date,
            value: v.value.parse::<f32>().ok(),
        })
        .collect()
}

async fn fred<T>(path: String) -> Result<T, YieldWatchError>
where
    T: DeserializeOwned,
{
    if !path.starts_with("/") {
        panic!("Invalid path!")
    }

    let response = HTTP
        .get(format!(
            "https://api.stlouisfed.org/fred{}{}api_key={}&file_type=json",
            path,
            if path.contains("?") { "&" } else { "?" },
            env::var("FRED_API_KEY").expect("FRED_API_KEY environment variable needs to be set!"),
        ))
        .send()
        .await?;

    // TODO: less performant than parsing to JSON directly
    let text_body = response.text().await.unwrap();

    Ok(
        // response
        // .json::<T>()
        serde_json::from_str::<T>(&text_body).unwrap_or_else(|error| {
            panic!("Failed to deserialize JSON: {} \n {}", text_body, error)
        }),
    )
}

// https://api.stlouisfed.org/fred/series/observations?series_id=DGS10&api_key=b4a0e80502479a066e859dacc9c044d0&file_type=json
