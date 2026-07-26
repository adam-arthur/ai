use futures::{StreamExt, stream};
use time::OffsetDateTime;

use crate::ingest_utils::{
    common::INGEST_SETTINGS, ensure_corporate_actions, ensure_data_folders, ensure_exchange_rates, ensure_prices, ensure_sectors, ensure_tradable_symbols, ensure_treasury_rates, is_valid_symbol
};

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
        .map(|v| v.symbol)
        .collect::<Vec<String>>();

    tokio::join!(
        populate_sectors(&tradable_symbols),
        populate_timeseries(&tradable_symbols),
    );

    // const [sectors] = await Promise.all([
    //     populatePortfolioMeta(tradableItems),
    // ])

    // TODO: Optimize this path
    // const symbolToSector = Object.fromEntries(sectors.map(v => [v.meta.symbol, v.value]))

    // FFO Only applies to equity reits
    // const equityReitSymbols = tradableSymbols.filter(s => {
    //     const sector = symbolToSector[s]
    //     const isEquityReit = sector && sector.industryName === 'Equity Real Estate Investment Trusts (REITs)'
    //     return isEquityReit
    // })
    // const chunkedEquityReitSymbols = chunk(equityReitSymbols, 5)
    // for (const symbolChunk of chunkedEquityReitSymbols) {
    //     const { wasCached } = await ensureFfoHistory({
    //         symbols: symbolChunk
    //     })

    //     if (!wasCached) {
    //         await sleep(30_000)
    //     }
    // }
}

async fn populate_sectors(tradable_symbols: &[String]) {
    log::info!(
        "Sectors - fetching data for {} symbols in chunks of {}...",
        tradable_symbols.len(),
        INGEST_SETTINGS.sa_fetch_chunk_size,
    );
    let chunked_symbols = tradable_symbols.chunks(INGEST_SETTINGS.sa_fetch_chunk_size as usize);

    let chunked_symbols_len = chunked_symbols.len();

    for (chunk_idx, chunk) in chunked_symbols.enumerate() {
        log::debug!(
            "Sectors - processing chunk {} of {}...",
            chunk_idx + 1,
            chunked_symbols_len
        );

        log::trace!("Sectors - Chunk - {:?}", chunk);
        ensure_sectors(chunk).await;
    }
}

async fn populate_timeseries(tradable_symbols: &[String]) {
    log::info!("Ingesting: Timeseries, Corporate Actions...");

    // ADAMTODO: 10 may be too high.. limited to 10_000 requests/min
    stream::iter(tradable_symbols.iter().enumerate())
        .for_each_concurrent(10, |(idx, symbol)| async move {
            log::info!("Processing: {}...", symbol);
            let start_time = OffsetDateTime::now_utc();
            tokio::join!(ensure_corporate_actions(symbol), ensure_prices(symbol),);

            let elapsed_time = OffsetDateTime::now_utc() - start_time;
            log::info!(
                "Stock {} of {} ({}%) ({}s)",
                idx + 1,
                tradable_symbols.len(),
                format!(
                    "{:.2}",
                    100.0f32 * ((idx + 1) as f32 / tradable_symbols.len() as f32)
                ),
                format!("{:.3}", elapsed_time.as_seconds_f32())
            );
        })
        .await
}
