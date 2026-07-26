// use std::collections::HashMap;

use async_trait::async_trait;
use futures::FutureExt;
use time::Duration;

use crate::{
    financials::{api::fetch_all_tradable_symbols, models::SymbolMeta}, ingest_utils::{
        common::{EnsureDataParams, ensure_data}, is_valid_symbol
    }, meta_utils::get_app_data_path
};

use super::common::EnsureDataResult;

// TODO: Preserve older/no longer tradable symbols
pub async fn ensure_tradable_symbols() -> EnsureDataResult<Vec<SymbolMeta>> {
    struct EnsureTradableSymbolsParams;

    #[async_trait]
    impl EnsureDataParams<Vec<SymbolMeta>> for EnsureTradableSymbolsParams {
        async fn get_fresh_data(&self, _cached_data: Option<Vec<SymbolMeta>>) -> Vec<SymbolMeta> {
            log::info!("TradableSymbols - fetching data...");

            let mut symbols = fetch_all_tradable_symbols()
                .map(|v: Vec<SymbolMeta>| {
                    v.into_iter()
                        .filter(|v| is_valid_symbol(&v.symbol))
                        .collect::<Vec<SymbolMeta>>()
                })
                .await;

            // ADAMTODO: Do the join of cik here
            log::debug!("Tradable Symbols: {}", symbols.len(),);

            symbols.sort_by(|a, b| a.symbol.cmp(&b.symbol));

            log::debug!("Formatted Symbols: {}", symbols.len());
            symbols
        }
        fn get_time_until_cache_is_stale(&self) -> Duration {
            Duration::hours(u8::MAX as i64)
        }
        fn get_file_path(&self) -> String {
            format!(
                "{}/{}",
                get_app_data_path().as_path().display(),
                "tradableSymbols.json"
            )
        }
    }

    let data = ensure_data(&EnsureTradableSymbolsParams).await;

    log::info!(
        "{}",
        if !data.was_cached {
            "TradableSymbols - writing data to cache..."
        } else {
            "TradableSymbols - data already exists, using cache..."
        }
    );

    data
}
