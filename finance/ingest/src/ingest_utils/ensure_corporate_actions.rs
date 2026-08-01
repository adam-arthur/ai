use async_trait::async_trait;
use time::Duration;

use crate::{
    common::alpaca_api::{CorporateActions, fetch_corporate_actions, merge_corporate_actions}, ingest_utils::common::ensure_data, meta_utils::get_app_data_path
};

use super::common::{EnsureDataParams, EnsureDataResult};

pub async fn ensure_corporate_actions(symbol: &String) -> EnsureDataResult<Vec<CorporateActions>> {
    struct EnsureCorporateActionsParams {
        symbol: String,
    }

    #[async_trait]
    impl EnsureDataParams<Vec<CorporateActions>> for EnsureCorporateActionsParams {
        async fn get_fresh_data(
            &self,
            cached_data: Option<Vec<CorporateActions>>,
        ) -> Vec<CorporateActions> {
            match cached_data {
                // If actions exist, use latest date, else arbitrary early date to fetch all
                Some(cached_data) if !cached_data.is_empty() => {
                    log::debug!(
                        "{} - Corporate Actions - fetching partial data...",
                        self.symbol
                    );

                    let new_corporate_actions =
                        fetch_corporate_actions(&self.symbol, &cached_data.last().unwrap().date)
                            .await;

                    merge_corporate_actions(cached_data, new_corporate_actions)
                }
                _ => {
                    log::debug!(
                        "{} - Corporate Actions - fetching fresh data...",
                        self.symbol
                    );
                    fetch_corporate_actions(&self.symbol, "2015-01-01").await
                }
            }
        }
        fn get_time_until_cache_is_stale(&self) -> Duration {
            Duration::hours(8)
        }
        fn get_file_path(&self) -> String {
            format!(
                "{}/corporateActions/{}.json",
                get_app_data_path().as_path().display(),
                self.symbol
            )
        }
    }

    let corporate_actions = ensure_data(&EnsureCorporateActionsParams {
        symbol: symbol.clone(),
    })
    .await;

    if !corporate_actions.was_cached {
        log::info!("{} - CorporateActions - writing data to cache...", symbol);
    } else {
        log::info!(
            "{} - CorporateActions - data already exists, using cache...",
            symbol
        );
    }

    corporate_actions
}
