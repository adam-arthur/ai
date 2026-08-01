use std::{fs::File, io::BufReader, path::Path};

use anyhow::{Context, Result};
use serde::de::DeserializeOwned;

use crate::{
    common::alpaca_api::CorporateActions, ingest_utils::common::FileSystemData, meta_utils::get_app_data_path
};

use super::models::{PricePoint, Sector, SymbolMeta};

pub fn read_tradable_symbols() -> Result<Vec<SymbolMeta>> {
    read_tradable_symbols_from(get_app_data_path())
}

pub fn read_tradable_symbols_from(data_path: &Path) -> Result<Vec<SymbolMeta>> {
    Ok(deserialize_json_file::<FileSystemData<Vec<SymbolMeta>>>(
        &data_path.join("tradableSymbols.json"),
    )?
    .value)
}

pub fn read_prices_from(data_path: &Path, symbol: &str) -> Result<Vec<PricePoint>> {
    Ok(deserialize_json_file::<FileSystemData<Vec<PricePoint>>>(
        &data_path.join("prices").join(format!("{symbol}.json")),
    )?
    .value)
}

// ADAMTODO: Rethink corporate action structure
pub fn read_corporate_actions_from(
    data_path: &Path,
    symbol: &str,
) -> Result<Vec<CorporateActions>> {
    Ok(
        deserialize_json_file::<FileSystemData<Vec<CorporateActions>>>(
            &data_path
                .join("corporateActions")
                .join(format!("{symbol}.json")),
        )?
        .value,
    )
}

pub fn read_sector_from(data_path: &Path, symbol: &str) -> Result<Sector> {
    Ok(deserialize_json_file::<FileSystemData<Sector>>(
        &data_path.join("sectors").join(format!("{symbol}.json")),
    )?
    .value)
}

fn deserialize_json_file<T: DeserializeOwned>(path: &Path) -> Result<T> {
    let file = File::open(path).with_context(|| format!("failed to open {}", path.display()))?;
    let reader = BufReader::new(file);
    serde_json::from_reader(reader)
        .with_context(|| format!("failed to deserialize {}", path.display()))
}
