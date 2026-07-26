use async_trait::async_trait;
use strum::IntoEnumIterator;
use time::Duration;

use crate::{
    common::fred_api::fetch_treasury_rates, financials::models::{TreasuryDuration, TreasuryRate}, ingest_utils::common::{EnsureDataParams, ensure_data}, meta_utils::get_app_data_path
};

// TODO: Support updating latest point
pub async fn ensure_treasury_rates() {
    struct EnsureTreasuryRates {
        duration: TreasuryDuration,
    }

    #[async_trait]
    impl EnsureDataParams<Vec<TreasuryRate>> for EnsureTreasuryRates {
        async fn get_fresh_data(
            &self,
            _cached_data: Option<Vec<TreasuryRate>>,
        ) -> Vec<TreasuryRate> {
            log::info!(
                "TreasuryRates - {} - fetching data...",
                self.duration.as_value()
            );
            fetch_treasury_rates(self.duration).await
        }
        fn get_time_until_cache_is_stale(&self) -> Duration {
            Duration::hours(8)
        }
        fn get_file_path(&self) -> String {
            format!(
                "{}/treasuries/treasuryRates_{}.json",
                get_app_data_path().as_path().display(),
                self.duration.as_value()
            )
        }
    }

    for duration in TreasuryDuration::iter() {
        let data = ensure_data::<Vec<TreasuryRate>>(&EnsureTreasuryRates { duration }).await;

        log::info!(
            "TreasuryRates - {} - {}",
            duration,
            if data.was_cached {
                "data already exists, using cache..."
            } else {
                "writing data to cache..."
            },
        );
    }
}
