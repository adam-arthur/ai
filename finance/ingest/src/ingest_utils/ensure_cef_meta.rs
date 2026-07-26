use std::collections::HashMap;

use async_trait::async_trait;
use time::Duration;

use crate::{
    common::cefconnect_api::fetch_symbol_to_cef_meta, financials::models::CefMeta, ingest_utils::common::{EnsureDataParams, ensure_data}, meta_utils::get_app_data_path
};

use super::common::EnsureDataResult;

#[allow(dead_code)]
pub async fn ensure_cef_meta() -> EnsureDataResult<HashMap<String, CefMeta>> {
    struct EnsureCefMeta {}

    #[async_trait]
    impl EnsureDataParams<HashMap<String, CefMeta>> for EnsureCefMeta {
        async fn get_fresh_data(
            &self,
            _cached_data: Option<HashMap<String, CefMeta>>,
        ) -> HashMap<String, CefMeta> {
            log::info!("CefMeta - fetching data...");
            fetch_symbol_to_cef_meta().await
        }
        fn get_time_until_cache_is_stale(&self) -> Duration {
            Duration::hours(8)
        }
        fn get_file_path(&self) -> String {
            format!(
                "{}/symbolToCefMeta.json",
                get_app_data_path().as_path().display()
            )
        }
    }

    let data = ensure_data::<HashMap<String, CefMeta>>(&EnsureCefMeta {}).await;

    log::info!(
        "CefMeta - {}",
        if data.was_cached {
            "data already exists, using cache..."
        } else {
            "writing data to cache..."
        },
    );

    data
}
