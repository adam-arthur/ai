use std::time::{Duration, Instant};

use futures::{StreamExt, stream};

use crate::financials::local_api::read_sector_from;
use crate::financials::models::SymbolMeta;
use crate::ingest_utils::{
    common::INGEST_SETTINGS, ensure_company, ensure_corporate_actions, ensure_data_folders, ensure_exchange_rates, ensure_ffo, ensure_prices, ensure_sectors, ensure_tradable_symbols, ensure_treasury_rates, is_valid_symbol
};
use crate::meta_utils::get_app_data_path;

const POPULATE_FFO: bool = false;

pub async fn ingest() {
    log::debug!("Settings {:?}", INGEST_SETTINGS);

    log::info!("Populate Data - Clearing out previous...");
    ensure_data_folders();

    log::info!("Populate Data - Updating metadata...");
    let (tradable_symbols_result, _, _ /*, cef_meta*/) = tokio::join!(
        ensure_tradable_symbols(),
        ensure_exchange_rates(),
        ensure_treasury_rates(),
        // ensure_cef_meta(),
    );

    log::info!("Populate Data - Updating stocks...");
    let tradable_symbols = tradable_symbols_result
        .value
        .into_iter()
        .filter(|v| is_valid_symbol(&v.symbol))
        .collect::<Vec<SymbolMeta>>();
    let symbols = tradable_symbols
        .iter()
        .map(|symbol_meta| symbol_meta.symbol.clone())
        .collect::<Vec<_>>();

    tokio::join!(
        populate_sectors(&symbols),
        populate_timeseries(&tradable_symbols),
    );

    if POPULATE_FFO {
        populate_ffo(&tradable_symbols).await;
    }
}

fn is_equity_reit_gics(sub_industry_gics: Option<u64>) -> bool {
    sub_industry_gics.is_some_and(|gics| gics / 10_000 == 6010)
}

async fn populate_ffo(tradable_symbols: &[SymbolMeta]) {
    log::info!("FFO - identifying equity REITs...");
    let data_path = get_app_data_path();

    let mut reits = Vec::new();
    for symbol_meta in tradable_symbols {
        let symbol = &symbol_meta.symbol;
        let sector = match read_sector_from(data_path, symbol) {
            Ok(sector) => sector,
            Err(error) => {
                log::warn!(
                    "{} - FFO - sector data unavailable, skipping: {error:#}",
                    symbol
                );
                continue;
            }
        };
        if !is_equity_reit_gics(sector.sub_industry_gics) {
            continue;
        }

        let Some(cik) = symbol_meta.cik.as_deref() else {
            log::warn!("{} - FFO - no CIK, skipping...", symbol);
            continue;
        };
        reits.push((symbol.as_str(), cik));
    }

    let mut progress = CompletionProgress::new("FFO", reits.len());
    let mut tasks = stream::iter(reits)
        .map(|(symbol, cik)| async move {
            let start_time = Instant::now();
            let result = ensure_ffo(symbol, cik).await;
            (symbol, start_time.elapsed(), result)
        })
        .buffer_unordered(INGEST_SETTINGS.symbol_concurrency);

    while let Some((symbol, elapsed, result)) = tasks.next().await {
        progress.log(symbol, elapsed, result.err().as_ref());
    }
}

async fn populate_sectors(tradable_symbols: &[String]) {
    log::info!(
        "Sectors - checking cached data for {} symbols...",
        tradable_symbols.len(),
    );
    ensure_sectors(tradable_symbols).await;
}

async fn populate_timeseries(tradable_symbols: &[SymbolMeta]) {
    log::info!("Ingesting: Companies, Timeseries, Corporate Actions...");

    let mut progress = CompletionProgress::new("Stock", tradable_symbols.len());
    let mut tasks = stream::iter(tradable_symbols)
        .map(|symbol_meta| async move {
            let symbol = &symbol_meta.symbol;
            let start_time = Instant::now();
            let company = async {
                if let Some(cik) = &symbol_meta.cik {
                    ensure_company(symbol, cik).await;
                } else {
                    log::debug!("{} - Company - no CIK, skipping...", symbol);
                }
            };
            tokio::join!(
                ensure_corporate_actions(symbol),
                ensure_prices(symbol),
                company
            );
            (symbol.as_str(), start_time.elapsed())
        })
        .buffer_unordered(INGEST_SETTINGS.symbol_concurrency);

    while let Some((symbol, elapsed)) = tasks.next().await {
        progress.log(symbol, elapsed, None);
    }
}

struct CompletionProgress {
    data_name: &'static str,
    completed: usize,
    total: usize,
}

impl CompletionProgress {
    fn new(data_name: &'static str, total: usize) -> Self {
        Self {
            data_name,
            completed: 0,
            total,
        }
    }

    fn log(&mut self, symbol: &str, elapsed: Duration, error: Option<&anyhow::Error>) {
        self.completed += 1;
        let percentage = if self.total == 0 {
            100.0
        } else {
            100.0 * self.completed as f32 / self.total as f32
        };
        let status = if error.is_some() {
            "failed"
        } else {
            "completed"
        };
        let message = format!(
            "{} - {} {} - {} of {} ({:.2}%) ({:.3}s)",
            self.data_name,
            status,
            symbol,
            self.completed,
            self.total,
            percentage,
            elapsed.as_secs_f32()
        );

        if let Some(error) = error {
            log::error!("{message}: {error:#}");
        } else {
            log::info!("{message}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::is_equity_reit_gics;

    #[test]
    fn recognizes_all_equity_reit_industries() {
        assert!(is_equity_reit_gics(Some(60_101_010)));
        assert!(is_equity_reit_gics(Some(60_103_010)));
        assert!(is_equity_reit_gics(Some(60_108_050)));
    }

    #[test]
    fn excludes_mortgage_reits_and_other_real_estate_companies() {
        assert!(!is_equity_reit_gics(Some(40_204_010)));
        assert!(!is_equity_reit_gics(Some(60_201_010)));
        assert!(!is_equity_reit_gics(None));
    }
}
