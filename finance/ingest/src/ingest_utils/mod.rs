pub mod common;
pub use ingest::ingest;

mod ensure_cef_meta;
mod ensure_company;
mod ensure_corporate_actions;
mod ensure_exchange_rates;
mod ensure_prices;
mod ensure_sectors;
mod ensure_tradable_symbols;
mod ensure_treasury_rates;
mod ingest;

use std::fs::create_dir_all;

use crate::meta_utils::get_app_data_path;

use ensure_corporate_actions::ensure_corporate_actions;
use ensure_exchange_rates::ensure_exchange_rates;
use ensure_prices::ensure_prices;
use ensure_sectors::ensure_sectors;
use ensure_tradable_symbols::ensure_tradable_symbols;
use ensure_treasury_rates::ensure_treasury_rates;

fn ensure_data_folders() {
    let paths = [
        "treasuries",
        "realEstate",
        "quotes",
        "companies",
        "corporateActions",
        "stats",
        "news",
        "prices",
        "navPrices",
        "dividends",
        "sectors",
        "splits",
        "ffo",
        "stockTypes",
        "saTickers",
        "meta",
        "portfolio",
        "bdc",
        "derived",
        "derived/stocks",
        "tmp",
    ];

    for path in paths {
        let folder_path = format!("{}/{}", get_app_data_path().as_path().display(), path);
        let _ = create_dir_all(folder_path);
    }
}

pub fn is_valid_symbol(symbol: &str) -> bool {
    let is_weird_stock = symbol.contains('.');
    let is_special_case = symbol.len() >= 5
        && (symbol.ends_with('G')
            || symbol.ends_with('H')
            || symbol.ends_with('I')
            || symbol.ends_with('M')
            || symbol.ends_with('N')
            || symbol.ends_with('O')
            || symbol.ends_with('P')
            || symbol.ends_with('Q')
            || symbol.ends_with('U')
            || symbol.ends_with('W')
            || symbol.ends_with('R'));
    !is_weird_stock && !is_special_case
}
