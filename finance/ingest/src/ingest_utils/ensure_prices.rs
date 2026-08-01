use std::ops::Add;

use async_trait::async_trait;
use time::{Duration, OffsetDateTime};

use crate::{
    common::alpaca_api::fetch_historical_prices, financials::models::PricePoint, ingest_utils::common::{EnsureDataParams, ensure_data, from_short_iso, to_iso}, meta_utils::get_app_data_path
};

use super::{DATA_FETCH_START_DATE, common::EnsureDataResult};

pub async fn ensure_prices(symbol: &String) -> EnsureDataResult<Vec<PricePoint>> {
    struct EnsurePricesParams {
        symbol: String,
    }

    #[async_trait]
    impl EnsureDataParams<Vec<PricePoint>> for EnsurePricesParams {
        async fn get_fresh_data(&self, cached_data: Option<Vec<PricePoint>>) -> Vec<PricePoint> {
            match cached_data {
                Some(mut cached_prices) if !cached_prices.is_empty() => {
                    log::debug!("{} - Prices - fetching partial data...", self.symbol);

                    let new_prices = fetch_historical_prices(
                        self.symbol.clone(),
                        to_iso(
                            &from_short_iso(&cached_prices.last().unwrap().date)
                                .add(Duration::days(1)), // Add one day to ensure no overlapping
                        ),
                        to_iso(&OffsetDateTime::now_utc()),
                    )
                    .await;

                    let last_cached_date = cached_prices.last().unwrap().date.clone();
                    for new_price in new_prices {
                        // TODO: Shouldn't be needed
                        if last_cached_date == new_price.date {
                            continue;
                        }
                        cached_prices.push(new_price);
                    }

                    cached_prices
                }
                _ => {
                    log::debug!("{} - Prices - fetching fresh data...", self.symbol);

                    fetch_historical_prices(
                        self.symbol.clone(),
                        DATA_FETCH_START_DATE.to_string(),
                        to_iso(&OffsetDateTime::now_utc()),
                    )
                    .await
                }
            }
        }
        fn get_time_until_cache_is_stale(&self) -> Duration {
            Duration::hours(8)
        }
        fn get_file_path(&self) -> String {
            format!(
                "{}/prices/{}.json",
                get_app_data_path().as_path().display(),
                self.symbol
            )
        }
    }

    let price_data = ensure_data(&EnsurePricesParams {
        symbol: symbol.clone(),
    })
    .await;

    if !price_data.was_cached {
        log::info!("{} - Prices - writing data to cache...", symbol);
    } else {
        log::info!("{} - Prices - data already exists, using cache...", symbol);
    }

    price_data
}
