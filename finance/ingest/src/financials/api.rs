use crate::common::{
    ALPACA, alpaca_api::{AssetStatus, GetAssetsArgs}, sec_api::fetch_symbol_to_cik
};

use super::models::SymbolMeta;

pub async fn fetch_all_tradable_symbols() -> Vec<SymbolMeta> {
    let symbol_to_cik = fetch_symbol_to_cik().await;

    ALPACA
        .get_assets(&GetAssetsArgs {
            status: Some(AssetStatus::Active),
            asset_class: Some("us_equity".into()),
        })
        .await
        .iter()
        .filter(|v| v.tradable)
        .map(|v| SymbolMeta {
            symbol: v.symbol.to_owned(),
            cik: symbol_to_cik.get(&v.symbol).map(|v| v.to_string()),
            name: v.name.to_owned(),
            exchange: v.exchange,
            is_easy_to_borrow: v.easy_to_borrow,
            is_shortable: v.shortable,
            is_fractionable: v.fractionable,
        })
        .collect()
}
