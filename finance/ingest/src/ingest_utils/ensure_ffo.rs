use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use time::{Duration, OffsetDateTime, format_description::well_known::Rfc3339};

use crate::{
    ffo::{ReitFfoExtraction, fetch_reit_ffo_data_to_cache}, file_utils::write_json_atomic, meta_utils::get_app_data_path
};

use super::common::{DataMeta, EnsureDataResult, FileSystemData, get_cached_data, is_stale};

const FFO_CACHE_LIFETIME: Duration = Duration::hours(24);

fn ffo_path(data_path: &Path, symbol: &str) -> PathBuf {
    data_path.join("ffo").join(format!("{symbol}.json"))
}

pub async fn ensure_ffo(symbol: &str, cik: &str) -> Result<EnsureDataResult<ReitFfoExtraction>> {
    let path = ffo_path(get_app_data_path(), symbol);
    let path_string = path.to_string_lossy();
    let cached_data = get_cached_data::<ReitFfoExtraction>(&path_string);

    if !is_stale(&cached_data, FFO_CACHE_LIFETIME)
        && let Some(cached_data) = cached_data
    {
        log::info!("{} - FFO - data already exists, using cache...", symbol);
        return Ok(EnsureDataResult {
            value: cached_data.value,
            was_cached: true,
        });
    }

    log::info!("{} - FFO - fetching SEC source filings...", symbol);
    let extraction = fetch_reit_ffo_data_to_cache(symbol, cik)
        .await
        .with_context(|| format!("failed to fetch FFO data for {symbol} (CIK {cik})"))?;
    let fresh_data = FileSystemData {
        meta: DataMeta {
            last_updated: OffsetDateTime::now_utc().format(&Rfc3339)?,
        },
        value: extraction,
    };
    write_json_atomic(&path, &fresh_data).with_context(|| {
        format!(
            "failed to write FFO data for {symbol} to {}",
            path.display()
        )
    })?;

    log::info!("{} - FFO - wrote computed data to cache...", symbol);
    Ok(EnsureDataResult {
        value: fresh_data.value,
        was_cached: false,
    })
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
}
