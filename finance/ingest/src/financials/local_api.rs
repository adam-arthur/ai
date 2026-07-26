use std::{fs::File, io::BufReader};

use serde::de::DeserializeOwned;

use crate::{
    common::alpaca_api::CorporateActions, ingest_utils::common::FileSystemData, meta_utils::get_app_data_path
};

use super::models::{PricePoint, Sector, SymbolMeta};

// ADAMTODO: move all to tokio/async
pub async fn read_tradable_symbols() -> Vec<SymbolMeta> {
    deserialize_json_file::<FileSystemData<Vec<SymbolMeta>>>(
        get_app_data_path()
            .join("tradableSymbols.json")
            .to_str()
            .unwrap(),
    )
    .value
}

pub async fn read_prices(symbol: &str) -> Vec<PricePoint> {
    deserialize_json_file::<FileSystemData<Vec<PricePoint>>>(
        get_app_data_path()
            .join(format!("/prices/{}.json", symbol))
            .to_str()
            .unwrap(),
    )
    .value
}

// ADAMTODO: Rethink corporate action structure
pub async fn read_corporate_actions(symbol: &str) -> Vec<CorporateActions> {
    deserialize_json_file::<FileSystemData<Vec<CorporateActions>>>(
        get_app_data_path()
            .join(format!("/corporateActions/{}.json", symbol))
            .to_str()
            .unwrap(),
    )
    .value
}

pub async fn read_sector(symbol: &str) -> Sector {
    deserialize_json_file::<FileSystemData<Sector>>(
        get_app_data_path()
            .join(format!("/sectors/{}.json", symbol))
            .to_str()
            .unwrap(),
    )
    .value
}

fn deserialize_json_file<T: DeserializeOwned>(path: &str) -> T {
    let file = File::open(path).unwrap_or_else(|err| {
        panic!("Failed to open file {}: {}", path, err);
    });
    let reader = BufReader::new(file);
    serde_json::from_reader(reader).unwrap_or_else(|err| {
        panic!("Failed to deserialize JSON from file {}: {}", path, err);
    })
}
