use async_trait::async_trait;
use time::Duration;

use crate::{
    financials::models::ExchangeRate, ingest_utils::common::{EnsureDataParams, ensure_data}, meta_utils::get_app_data_path
};

use super::common::EnsureDataResult;

pub async fn ensure_exchange_rates() -> EnsureDataResult<Vec<ExchangeRate>> {
    struct EnsureExchangeRates;

    #[async_trait]
    impl EnsureDataParams<Vec<ExchangeRate>> for EnsureExchangeRates {
        async fn get_fresh_data(
            &self,
            _cached_data: Option<Vec<ExchangeRate>>,
        ) -> Vec<ExchangeRate> {
            log::debug!("ExchangeRates - fetching data...");
            vec![]
            // fetch_exchange_rates().await
        }
        fn get_time_until_cache_is_stale(&self) -> Duration {
            Duration::hours(8)
        }
        fn get_file_path(&self) -> String {
            format!(
                "{}/{}",
                get_app_data_path().as_path().display(),
                "exchangeRates.json"
            )
        }
    }

    let data = ensure_data(&EnsureExchangeRates).await;

    log::debug!(
        "{}",
        if !data.was_cached {
            "ExchangeRates - writing data to cache..."
        } else {
            "ExchangeRates - data already exists, using cache..."
        }
    );

    data
}
