use async_trait::async_trait;
use time::Duration;

use crate::{
    common::sec_api::fetch_company, financials::models::Company, ingest_utils::common::{EnsureDataParams, ensure_optional_data}, meta_utils::get_app_data_path
};

use super::common::EnsureDataResult;

pub async fn ensure_company(symbol: &str, cik: &str) -> EnsureDataResult<Option<Company>> {
    struct EnsureCompanyParams {
        symbol: String,
        cik: String,
    }

    #[async_trait]
    impl EnsureDataParams<Option<Company>> for EnsureCompanyParams {
        async fn get_fresh_data(&self, cached_data: Option<Option<Company>>) -> Option<Company> {
            log::debug!("{} - Company - fetching data...", self.symbol);
            match fetch_company(&self.symbol, &self.cik).await {
                Ok(company) => Some(company),
                Err(error) => {
                    log::error!("{} - failed to fetch SEC company: {error}", self.symbol);
                    cached_data.flatten()
                }
            }
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

    let company = ensure_optional_data(&EnsureCompanyParams {
        symbol: symbol.to_owned(),
        cik: cik.to_owned(),
    })
    .await;

    if company.value.is_none() {
        log::warn!("{} - Company - SEC data unavailable", symbol);
    } else if !company.was_cached {
        log::info!("{} - Company - writing data to cache...", symbol);
    } else {
        log::info!("{} - Company - data already exists, using cache...", symbol);
    }

    company
}
