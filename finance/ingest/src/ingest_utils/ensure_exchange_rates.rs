use async_trait::async_trait;
use time::Duration;

use crate::{
    common::ecb_api::fetch_exchange_rates, financials::models::ExchangeRateSnapshot, ingest_utils::common::{EnsureDataParams, ensure_data}, meta_utils::get_app_data_path
};

use super::common::EnsureDataResult;

const EXCHANGE_RATE_HISTORY_START: &str = "2016-01-01";

pub async fn ensure_exchange_rates() -> EnsureDataResult<Vec<ExchangeRateSnapshot>> {
    struct EnsureExchangeRates;

    #[async_trait]
    impl EnsureDataParams<Vec<ExchangeRateSnapshot>> for EnsureExchangeRates {
        async fn get_fresh_data(
            &self,
            cached_data: Option<Vec<ExchangeRateSnapshot>>,
        ) -> Vec<ExchangeRateSnapshot> {
            let cached_snapshots = cached_data.as_deref().unwrap_or_default();
            let start_period = exchange_rate_start_period(cached_snapshots);
            log::debug!(
                "ExchangeRates - fetching {} data...",
                if !cached_snapshots.is_empty() {
                    "partial"
                } else {
                    "historical"
                }
            );

            let fresh_data = fetch_exchange_rates(start_period)
                .await
                .unwrap_or_else(|error| panic!("Failed to fetch ECB exchange rates: {error:#}"));

            merge_exchange_rates(cached_data.unwrap_or_default(), fresh_data)
        }
        fn get_time_until_cache_is_stale(&self) -> Duration {
            Duration::hours(20)
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

fn exchange_rate_start_period(snapshots: &[ExchangeRateSnapshot]) -> &str {
    snapshots
        .last()
        .map_or(EXCHANGE_RATE_HISTORY_START, |snapshot| {
            snapshot.as_of.as_str()
        })
}

fn merge_exchange_rates(
    cached_data: Vec<ExchangeRateSnapshot>,
    fresh_data: Vec<ExchangeRateSnapshot>,
) -> Vec<ExchangeRateSnapshot> {
    let mut by_date = cached_data
        .into_iter()
        .map(|snapshot| (snapshot.as_of.clone(), snapshot))
        .collect::<std::collections::BTreeMap<_, _>>();

    for snapshot in fresh_data {
        by_date.insert(snapshot.as_of.clone(), snapshot);
    }

    by_date.into_values().collect()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::financials::models::Currency;

    use super::*;

    fn snapshot(as_of: &str, usd_rate: f64) -> ExchangeRateSnapshot {
        ExchangeRateSnapshot {
            as_of: as_of.into(),
            base: Currency::EUR,
            rates: BTreeMap::from([(Currency::EUR, 1.0), (Currency::USD, usd_rate)]),
        }
    }

    #[test]
    fn starts_exchange_rate_history_in_2016() {
        assert_eq!(exchange_rate_start_period(&[]), "2016-01-01");
    }

    #[test]
    fn resumes_exchange_rate_history_from_latest_snapshot() {
        let snapshots = vec![snapshot("2026-07-29", 1.1), snapshot("2026-07-30", 1.2)];

        assert_eq!(exchange_rate_start_period(&snapshots), "2026-07-30");
    }

    #[test]
    fn merges_new_data_and_replaces_revisions() {
        let merged = merge_exchange_rates(
            vec![snapshot("2026-07-29", 1.1), snapshot("2026-07-30", 1.2)],
            vec![snapshot("2026-07-30", 1.25), snapshot("2026-07-31", 1.3)],
        );

        assert_eq!(merged.len(), 3);
        assert_eq!(merged[0].as_of, "2026-07-29");
        assert_eq!(merged[1].rates[&Currency::USD], 1.25);
        assert_eq!(merged[2].as_of, "2026-07-31");
    }
}
