use futures::{StreamExt, stream};
use time::OffsetDateTime;

use crate::financials::local_api::read_sector_from;
use crate::financials::models::SymbolMeta;
use crate::ingest_utils::{
    common::INGEST_SETTINGS, ensure_company, ensure_corporate_actions, ensure_data_folders, ensure_exchange_rates, ensure_ffo, ensure_prices, ensure_sectors, ensure_tradable_symbols, ensure_treasury_rates, is_valid_symbol
};
use crate::meta_utils::get_app_data_path;

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

    populate_ffo(&tradable_symbols).await;
}

fn is_equity_reit_gics(sub_industry_gics: Option<u64>) -> bool {
    sub_industry_gics.is_some_and(|gics| gics / 10_000 == 6010)
}

async fn populate_ffo(tradable_symbols: &[SymbolMeta]) {
    log::info!("FFO - identifying equity REITs...");
    let data_path = get_app_data_path();

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
        if let Err(error) = ensure_ffo(symbol, cik).await {
            log::error!("{} - FFO - ingest failed: {error:#}", symbol);
        }
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

    // ADAMTODO: 10 may be too high.. limited to 10_000 requests/min
    stream::iter(tradable_symbols.iter().enumerate())
        .for_each_concurrent(10, |(idx, symbol_meta)| async move {
            let symbol = &symbol_meta.symbol;
            log::info!("Processing: {}...", symbol);
            let start_time = OffsetDateTime::now_utc();
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

            let elapsed_time = OffsetDateTime::now_utc() - start_time;
            log::info!(
                "Stock {} of {} ({:.2}%) ({:.3}s)",
                idx + 1,
                tradable_symbols.len(),
                100.0f32 * ((idx + 1) as f32 / tradable_symbols.len() as f32),
                elapsed_time.as_seconds_f32()
            );
        })
        .await
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
