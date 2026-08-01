use std::collections::HashMap;

use async_trait::async_trait;
use time::Duration;

use crate::{
    common::seekingalpha_api::fetch_sector_quotes, financials::models::Sector, ingest_utils::common::{EnsureBatchedDataParams, ensure_batched_data}, meta_utils::get_app_data_path
};

pub async fn ensure_sectors(symbols: &[String]) -> bool {
    struct EnsureBatchedSectorsParams<'a> {
        symbols: &'a [String],
    }

    #[async_trait]
    impl EnsureBatchedDataParams<Sector> for EnsureBatchedSectorsParams<'_> {
        async fn get_fresh_data(&self, stale_symbols: &[String]) -> HashMap<String, Sector> {
            log::debug!(
                "Sectors - chunk has {} stale symbols...",
                stale_symbols.len()
            );
            fetch_sector_quotes(stale_symbols).await
        }
        fn get_time_until_cache_is_stale(&self) -> Duration {
            Duration::hours(u8::MAX as i64)
        }
        fn get_symbols(&self) -> &[String] {
            self.symbols
        }
        fn get_file_path(&self, symbol: &str) -> String {
            format!(
                "{}/sectors/{}.json",
                get_app_data_path().as_path().display(),
                symbol
            )
        }
    }

    ensure_batched_data(&EnsureBatchedSectorsParams { symbols }).await

    // TODO: Return values
}
