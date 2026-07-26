use std::collections::HashMap;

use serde::{Deserialize, de::DeserializeOwned};

use crate::meta_utils::YieldWatchError;

use super::HTTP;

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

async fn sec<T>(path: String) -> Result<T, YieldWatchError>
where
    T: DeserializeOwned,
{
    if !path.starts_with('/') {
        panic!("Invalid path!")
    }

    let response = HTTP
        .get(format!("https://www.sec.gov{}", path,))
        .header("User-Agent", "Adam's Company adam.arthur.wilson@gmail.com")
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
