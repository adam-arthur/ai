use std::{collections::BTreeMap, path::Path};

use time::{Duration, OffsetDateTime, format_description::well_known::Rfc3339};

use crate::{
    common::treasury_api::{fetch_treasury_rate_history, fetch_treasury_rates}, file_utils::write_json_atomic, financials::models::{TreasuryDuration, TreasuryRate}, ingest_utils::common::{DataMeta, FileSystemData, get_cached_data, is_stale}, meta_utils::get_app_data_path
};

const TREASURY_HISTORY_START_YEAR: i32 = 2016;

pub async fn ensure_treasury_rates() {
    let mut cached = TreasuryDuration::ALL
        .into_iter()
        .map(|duration| {
            let data = get_cached_data::<Vec<TreasuryRate>>(&cache_path(duration));
            (duration, data)
        })
        .collect::<BTreeMap<_, _>>();

    if cached
        .values()
        .all(|data| !is_stale(data, Duration::hours(8)))
    {
        log::info!("TreasuryRates - data already exists, using cache...");
        return;
    }

    let has_complete_cache = cached.values().all(Option::is_some);
    log::info!(
        "TreasuryRates - fetching {} data for all maturities...",
        if has_complete_cache {
            "incremental"
        } else {
            "historical"
        }
    );
    let now = OffsetDateTime::now_utc();
    let mut fresh = if has_complete_cache {
        fetch_treasury_rates(&format!("{:04}{:02}", now.year(), u8::from(now.month()))).await
    } else {
        fetch_treasury_rate_history(TREASURY_HISTORY_START_YEAR, now.year()).await
    }
    .unwrap_or_else(|error| panic!("Failed to fetch Treasury rates: {error:#}"));
    let last_updated = OffsetDateTime::now_utc().format(&Rfc3339).unwrap();

    for duration in TreasuryDuration::ALL {
        let cached_rates = cached
            .remove(&duration)
            .flatten()
            .map(|data| data.value)
            .unwrap_or_default();
        let fresh_rates = fresh.remove(&duration).unwrap_or_default();
        let merged = merge_treasury_rates(cached_rates, fresh_rates);
        let path = cache_path(duration);
        write_json_atomic(
            Path::new(&path),
            &FileSystemData {
                meta: DataMeta {
                    last_updated: last_updated.clone(),
                },
                value: merged,
            },
        )
        .unwrap_or_else(|error| panic!("Failed to write Treasury cache file {path}: {error:#}"));
    }
}

fn cache_path(duration: TreasuryDuration) -> String {
    format!(
        "{}/treasuries/treasuryRates_{}.json",
        get_app_data_path().as_path().display(),
        duration.as_value()
    )
}

fn merge_treasury_rates(cached: Vec<TreasuryRate>, fresh: Vec<TreasuryRate>) -> Vec<TreasuryRate> {
    let mut by_date = cached
        .into_iter()
        .map(|rate| (rate.date.clone(), rate))
        .collect::<BTreeMap<_, _>>();
    for rate in fresh {
        by_date.insert(rate.date.clone(), rate);
    }
    by_date.into_values().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_replaces_refetched_dates_and_preserves_history() {
        let merged = merge_treasury_rates(
            vec![
                TreasuryRate {
                    date: "2026-06-30".into(),
                    value: Some(4.0),
                },
                TreasuryRate {
                    date: "2026-07-01".into(),
                    value: Some(4.1),
                },
            ],
            vec![
                TreasuryRate {
                    date: "2026-07-01".into(),
                    value: Some(4.2),
                },
                TreasuryRate {
                    date: "2026-07-02".into(),
                    value: Some(4.3),
                },
            ],
        );

        assert_eq!(merged.len(), 3);
        assert_eq!(merged[1].value, Some(4.2));
        assert_eq!(merged[2].date, "2026-07-02");
    }
}
