use async_trait::async_trait;
use time::Duration;

use crate::{
    financials::models::Company, ingest_utils::common::{EnsureDataParams, ensure_data}, meta_utils::get_app_data_path
};

use super::common::EnsureDataResult;

#[allow(dead_code)]
pub async fn ensure_company(symbol: &String) -> EnsureDataResult<Option<Company>> {
    struct EnsureCompanyParams {
        symbol: String,
    }

    #[async_trait]
    impl EnsureDataParams<Option<Company>> for EnsureCompanyParams {
        async fn get_fresh_data(&self, _cached_data: Option<Option<Company>>) -> Option<Company> {
            log::debug!("{} - Company - fetching data...", self.symbol);
            // fetch_company(&self.symbol).await
            None
        }
        fn get_time_until_cache_is_stale(&self) -> Duration {
            Duration::hours(u8::MAX as i64)
        }
        fn get_file_path(&self) -> String {
            format!(
                "{}/companies/{}.json",
                get_app_data_path().as_path().display(),
                self.symbol
            )
        }
    }

    let company = ensure_data(&EnsureCompanyParams {
        symbol: symbol.clone(),
    })
    .await;

    if !company.was_cached {
        log::info!("{} - Company - writing data to cache...", symbol);
    } else {
        log::info!("{} - Company - data already exists, using cache...", symbol);
    }

    company
}
