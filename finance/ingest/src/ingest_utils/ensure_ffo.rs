use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    fs, path::{Path, PathBuf}
};
use time::{Duration, OffsetDateTime, format_description::well_known::Rfc3339};

use crate::{
    ffo::{ReitFfoData, fetch_reit_ffo_data_to_cache}, file_utils::write_json_atomic, meta_utils::get_app_data_path
};

use super::common::EnsureDataResult;

const FFO_CACHE_LIFETIME: Duration = Duration::hours(24);

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct FfoFile {
    updated_at: String,
    #[serde(flatten)]
    data: ReitFfoData,
}

fn ffo_path(data_path: &Path, symbol: &str) -> PathBuf {
    data_path.join("ffo").join(format!("{symbol}.json"))
}

pub async fn ensure_ffo(symbol: &str, cik: &str) -> Result<EnsureDataResult<ReitFfoData>> {
    let path = ffo_path(get_app_data_path(), symbol);
    if let Some(cached) = read_ffo_file(&path)
        && is_fresh(&cached.updated_at)
    {
        log::info!("{} - FFO - data already exists, using cache...", symbol);
        return Ok(EnsureDataResult {
            value: cached.data,
            was_cached: true,
        });
    }

    log::info!("{} - FFO - fetching SEC source filings...", symbol);
    let data = fetch_reit_ffo_data_to_cache(symbol, cik)
        .await
        .with_context(|| format!("failed to fetch FFO data for {symbol} (CIK {cik})"))?;
    let file = FfoFile {
        updated_at: OffsetDateTime::now_utc().format(&Rfc3339)?,
        data,
    };
    write_json_atomic(&path, &file).with_context(|| {
        format!(
            "failed to write FFO data for {symbol} to {}",
            path.display()
        )
    })?;

    log::info!("{} - FFO - wrote computed data to cache...", symbol);
    Ok(EnsureDataResult {
        value: file.data,
        was_cached: false,
    })
}

fn read_ffo_file(path: &Path) -> Option<FfoFile> {
    let contents = fs::read_to_string(path).ok()?;
    serde_json::from_str(&contents).ok()
}

fn is_fresh(updated_at: &str) -> bool {
    OffsetDateTime::parse(updated_at, &Rfc3339)
        .is_ok_and(|updated_at| OffsetDateTime::now_utc() - updated_at < FFO_CACHE_LIFETIME)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn symbol_keys_the_computed_ffo_path() {
        assert_eq!(
            ffo_path(Path::new("data"), "VICI"),
            Path::new("data/ffo/VICI.json")
        );
    }

    #[test]
    fn persisted_shape_has_a_flat_issuer_header() {
        let file = FfoFile {
            updated_at: "2026-08-01T00:00:00Z".to_owned(),
            data: ReitFfoData {
                symbol: "AHR".to_owned(),
                cik: "1632970".to_owned(),
                periods: Vec::new(),
            },
        };
        let json = serde_json::to_value(file).unwrap();
        assert_eq!(json["symbol"], "AHR");
        assert!(json.get("updatedAt").is_some());
        assert!(json.get("meta").is_none());
        assert!(json.get("value").is_none());
    }
}
