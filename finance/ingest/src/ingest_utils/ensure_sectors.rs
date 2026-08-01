use std::collections::HashMap;

use async_trait::async_trait;
use time::Duration;

use crate::{
    common::seekingalpha_api::fetch_sector_quotes, financials::models::Sector, ingest_utils::common::{EnsureBatchedDataParams, INGEST_SETTINGS, ensure_batched_data}, meta_utils::get_app_data_path
};

pub async fn ensure_sectors(symbols: &[String]) -> bool {
    struct EnsureBatchedSectorsParams<'a> {
        symbols: &'a [String],
    }

    #[async_trait]
    impl EnsureBatchedDataParams<Sector> for EnsureBatchedSectorsParams<'_> {
        async fn get_fresh_data(&self, stale_symbols: &[String]) -> HashMap<String, Sector> {
            fetch_sector_quotes(stale_symbols).await
        }
        fn get_batch_size(&self) -> usize {
            INGEST_SETTINGS.sa_fetch_chunk_size as usize
        }
        fn get_data_name(&self) -> &str {
            "Sectors"
        }
        fn get_time_until_cache_is_stale(&self) -> Duration {
            // GICS classifications rarely change. Keep an existing classification until it is
            // explicitly removed so routine ingests only ask Seeking Alpha about new symbols.
            Duration::MAX
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
