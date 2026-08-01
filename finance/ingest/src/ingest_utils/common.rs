use std::{collections::HashMap, fs, path::Path};

use async_trait::async_trait;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use time::{
    Date, Duration, OffsetDateTime, UtcOffset, format_description::{FormatItem, parse, well_known::Rfc3339}
};

use crate::file_utils::write_json_atomic;

pub static SHORT_ISO_PARSER: Lazy<Vec<FormatItem<'_>>> =
    Lazy::new(|| parse("[year]-[month]-[day]").expect("Date format template is invalid!"));

/** Provides a mechanism for fetching and caching data for one entity at at time */
#[async_trait]
pub trait EnsureDataParams<T> {
    async fn get_fresh_data(&self, cached_data: Option<T>) -> T;
    fn get_file_path(&self) -> String;
    fn get_time_until_cache_is_stale(&self) -> Duration;
}

/** Provides a mechanism for fetching data for multiple entities and caching data separately for each one */
#[async_trait]
pub trait EnsureBatchedDataParams<T> {
    async fn get_fresh_data(&self, stale_data: &[String]) -> HashMap<String, T>;
    fn get_batch_size(&self) -> usize;
    fn get_data_name(&self) -> &str;
    fn get_symbols(&self) -> &[String];
    fn get_file_path(&self, symbol: &str) -> String;
    fn get_time_until_cache_is_stale(&self) -> Duration;
}

#[derive(Serialize, Deserialize, Debug)]
pub struct DataMeta {
    pub last_updated: String, // "2022-01-11T08:34:58.346-06:00"
}

// TODO: Should be two types
pub struct EnsureDataResult<T> {
    pub value: T,
    pub was_cached: bool,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct FileSystemData<T> {
    pub meta: DataMeta,
    pub value: T,
}

// Get cached data or refresh
pub async fn ensure_data<T>(params: &dyn EnsureDataParams<T>) -> EnsureDataResult<T>
where
    T: Serialize + DeserializeOwned,
{
    let cached_data = get_cached_data::<T>(&params.get_file_path());
    let is_up_to_date: bool =
        cached_data.is_some() && !is_stale(&cached_data, params.get_time_until_cache_is_stale());

    if is_up_to_date {
        log::trace!("ensure_data - cached data is up to date");
        let cached_data = cached_data.unwrap();
        return EnsureDataResult {
            value: cached_data.value,
            was_cached: true,
        };
    }

    log::trace!("ensure_data - cached data was not up to date, fetching new...");
    let fresh_data = FileSystemData::<T> {
        meta: DataMeta {
            last_updated: OffsetDateTime::now_utc().format(&Rfc3339).unwrap(),
        },
        // TODO make async
        value: params.get_fresh_data(cached_data.map(|v| v.value)).await,
    };

    let path = params.get_file_path();
    write_json_atomic(Path::new(&path), &fresh_data)
        .unwrap_or_else(|error| panic!("Failed to write cache file {path}: {error:#}"));

    EnsureDataResult {
        value: fresh_data.value,
        was_cached: false,
    }
}

/// Get cached optional data or refresh it. A cached `null` is treated as missing so a
/// temporary upstream gap does not become a long-lived negative cache entry.
pub async fn ensure_optional_data<T>(
    params: &dyn EnsureDataParams<Option<T>>,
) -> EnsureDataResult<Option<T>>
where
    T: Serialize + DeserializeOwned,
{
    let cached_data = get_cached_data::<Option<T>>(&params.get_file_path());
    let is_up_to_date = cached_data
        .as_ref()
        .is_some_and(|data| data.value.is_some())
        && !is_stale(&cached_data, params.get_time_until_cache_is_stale());

    if is_up_to_date {
        let cached_data = cached_data.unwrap();
        return EnsureDataResult {
            value: cached_data.value,
            was_cached: true,
        };
    }

    let fresh_data = FileSystemData::<Option<T>> {
        meta: DataMeta {
            last_updated: OffsetDateTime::now_utc().format(&Rfc3339).unwrap(),
        },
        value: params
            .get_fresh_data(cached_data.map(|data| data.value))
            .await,
    };

    let path = params.get_file_path();
    write_json_atomic(Path::new(&path), &fresh_data)
        .unwrap_or_else(|error| panic!("Failed to write cache file {path}: {error:#}"));

    EnsureDataResult {
        value: fresh_data.value,
        was_cached: false,
    }
}

pub async fn ensure_batched_data<T>(params: &dyn EnsureBatchedDataParams<T>) -> bool
where
    T: Serialize + DeserializeOwned,
{
    let batch_size = params.get_batch_size();
    let data_name = params.get_data_name();
    assert!(batch_size > 0, "batch size must be greater than zero");

    let last_updated = OffsetDateTime::now_utc().format(&Rfc3339).unwrap();
    // TODO: handle None case
    let stale_symbols: Vec<String> = params
        .get_symbols()
        .iter()
        .filter(|symbol| {
            let cached_sector = get_cached_data::<T>(&params.get_file_path(symbol));
            is_stale(&cached_sector, params.get_time_until_cache_is_stale())
        })
        .cloned()
        .collect();

    if stale_symbols.is_empty() {
        log::debug!("{data_name} - all cached data is up to date");
        return false;
    }

    let batch_count = stale_symbols.len().div_ceil(batch_size);
    log::debug!(
        "{data_name} - found {} stale symbols across {} API batches...",
        stale_symbols.len(),
        batch_count
    );

    for (batch_idx, stale_batch) in stale_symbols.chunks(batch_size).enumerate() {
        log::debug!(
            "{data_name} - processing API batch {} of {} ({} symbols)...",
            batch_idx + 1,
            batch_count,
            stale_batch.len()
        );
        log::trace!("{data_name} - API batch - {stale_batch:?}");

        let fresh_batched_data = params.get_fresh_data(stale_batch).await;
        for (symbol, data) in fresh_batched_data {
            let path = params.get_file_path(&symbol);
            write_json_atomic(
                Path::new(&path),
                &FileSystemData::<T> {
                    meta: DataMeta {
                        last_updated: last_updated.clone(),
                    },
                    value: data,
                },
            )
            .unwrap_or_else(|error| panic!("Failed to write cache file {path}: {error:#}"));
        }
    }

    // TODO: Parity with ensure_data method
    !stale_symbols.is_empty()
}

pub(crate) fn is_stale<T>(
    data: &Option<FileSystemData<T>>,
    time_until_cache_is_stale: Duration,
) -> bool {
    if data.is_none() {
        return true;
    }

    get_time_ago(&data.as_ref().unwrap().meta.last_updated) >= time_until_cache_is_stale
}

fn get_time_ago(iso_time: &str) -> Duration {
    OffsetDateTime::now_utc()
        - OffsetDateTime::parse(iso_time, &Rfc3339).expect("Failed to parse date-time")
}

pub(crate) fn get_cached_data<T>(cached_path: &str) -> Option<FileSystemData<T>>
where
    T: DeserializeOwned,
{
    log::trace!("get_cached_data - Searching for \"{}\"", cached_path);

    let does_file_exist = Path::new(cached_path).exists();
    if !does_file_exist {
        log::trace!("get_cached_data - Not found \"{}\"", cached_path);
        return None;
    }

    log::trace!("get_cached_data - Found \"{}\"", cached_path);

    let file_contents = fs::read_to_string(cached_path)
        .unwrap_or_else(|_| panic!("Failed to read file: {}", cached_path));

    let value: FileSystemData<T> = serde_json::from_str(&file_contents).unwrap_or_else(|error| {
        panic!(
            "Failed to deserialize file contents: {} \n {} \n {}",
            file_contents, error, cached_path
        )
    });

    Some(value)
}

pub fn parse_iso(date_str: &str) -> OffsetDateTime {
    OffsetDateTime::parse(date_str, &Rfc3339).expect("Failed to parse date-time")
}

pub fn parse_short_iso(date_str: &str) -> Date {
    Date::parse(date_str, &SHORT_ISO_PARSER).expect("Failed to parse date-time")
}

pub fn to_end_of_day(date_str: &str) -> String {
    parse_iso(date_str)
        .replace_time(time::macros::time!(23:59:59))
        .format(&Rfc3339)
        .expect("Failed to format date-time")
}

#[allow(dead_code)]
pub fn from_iso(date_str: &str) -> OffsetDateTime {
    OffsetDateTime::parse(date_str, &Rfc3339)
        .expect("Failed to parse string into date from ISO format")
}

pub fn from_short_iso(date_str: &str) -> OffsetDateTime {
    Date::parse(date_str, &SHORT_ISO_PARSER)
        .expect("Failed to parse string into date from [year]-[month]-[day] format")
        .midnight()
        .assume_offset(UtcOffset::from_whole_seconds(0).unwrap())
}

pub fn to_iso(date: &OffsetDateTime) -> String {
    date.format(&Rfc3339)
        .expect("Failed to format date-time")
        .to_string()
}

pub fn str_to_short_iso(date_str: &str) -> String {
    parse_iso(date_str)
        .format(&SHORT_ISO_PARSER)
        .expect("Failed to format date-time")
}

#[derive(Debug)]
pub struct IngestSettings {
    pub symbol_concurrency: usize,
    pub sa_throttle_duration: std::time::Duration,
    pub sa_fetch_chunk_size: u16,
}

pub const INGEST_SETTINGS: IngestSettings = IngestSettings {
    symbol_concurrency: 4,
    sa_throttle_duration: std::time::Duration::from_secs(180),
    sa_fetch_chunk_size: 50,
};

#[cfg(test)]
mod tests {
    use std::{
        sync::{
            Arc, Mutex, atomic::{AtomicUsize, Ordering}
        }, time::{SystemTime, UNIX_EPOCH}
    };

    use super::*;

    struct OptionalParams {
        path: String,
        fetch_count: Arc<AtomicUsize>,
    }

    struct BatchedParams {
        cache_dir: String,
        symbols: Vec<String>,
        fetched_batches: Arc<Mutex<Vec<Vec<String>>>>,
    }

    #[async_trait]
    impl EnsureBatchedDataParams<String> for BatchedParams {
        async fn get_fresh_data(&self, stale_data: &[String]) -> HashMap<String, String> {
            self.fetched_batches
                .lock()
                .unwrap()
                .push(stale_data.to_vec());
            stale_data
                .iter()
                .map(|symbol| (symbol.clone(), format!("fresh-{symbol}")))
                .collect()
        }

        fn get_batch_size(&self) -> usize {
            3
        }

        fn get_data_name(&self) -> &str {
            "Test data"
        }

        fn get_symbols(&self) -> &[String] {
            &self.symbols
        }

        fn get_file_path(&self, symbol: &str) -> String {
            format!("{}/{symbol}.json", self.cache_dir)
        }

        fn get_time_until_cache_is_stale(&self) -> Duration {
            Duration::MAX
        }
    }

    #[async_trait]
    impl EnsureDataParams<Option<String>> for OptionalParams {
        async fn get_fresh_data(&self, _cached_data: Option<Option<String>>) -> Option<String> {
            self.fetch_count.fetch_add(1, Ordering::SeqCst);
            Some("fresh".to_owned())
        }

        fn get_file_path(&self) -> String {
            self.path.clone()
        }

        fn get_time_until_cache_is_stale(&self) -> Duration {
            Duration::days(1)
        }
    }

    #[tokio::test]
    async fn optional_cache_refreshes_null_then_reuses_value() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "finance-optional-cache-{}-{unique}.json",
            std::process::id()
        ));
        fs::write(
            &path,
            serde_json::to_vec(&FileSystemData::<Option<String>> {
                meta: DataMeta {
                    last_updated: OffsetDateTime::now_utc().format(&Rfc3339).unwrap(),
                },
                value: None,
            })
            .unwrap(),
        )
        .unwrap();

        let fetch_count = Arc::new(AtomicUsize::new(0));
        let params = OptionalParams {
            path: path.to_string_lossy().into_owned(),
            fetch_count: fetch_count.clone(),
        };

        let refreshed = ensure_optional_data(&params).await;
        assert_eq!(refreshed.value.as_deref(), Some("fresh"));
        assert!(!refreshed.was_cached);

        let cached = ensure_optional_data(&params).await;
        assert_eq!(cached.value.as_deref(), Some("fresh"));
        assert!(cached.was_cached);
        assert_eq!(fetch_count.load(Ordering::SeqCst), 1);
        assert!(
            !path
                .with_file_name(format!(
                    ".{}.tmp",
                    path.file_name().unwrap().to_string_lossy()
                ))
                .exists()
        );

        fs::remove_file(path).unwrap();
    }

    #[tokio::test]
    async fn batched_cache_compacts_stale_symbols_before_fetching() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let cache_dir = std::env::temp_dir().join(format!(
            "finance-batched-cache-{}-{unique}",
            std::process::id()
        ));
        fs::create_dir(&cache_dir).unwrap();

        for symbol in ["A", "C", "F"] {
            fs::write(
                cache_dir.join(format!("{symbol}.json")),
                serde_json::to_vec(&FileSystemData {
                    meta: DataMeta {
                        last_updated: OffsetDateTime::now_utc().format(&Rfc3339).unwrap(),
                    },
                    value: format!("cached-{symbol}"),
                })
                .unwrap(),
            )
            .unwrap();
        }

        let fetched_batches = Arc::new(Mutex::new(Vec::new()));
        let params = BatchedParams {
            cache_dir: cache_dir.to_string_lossy().into_owned(),
            symbols: ["A", "B", "C", "D", "E", "F", "G"]
                .map(str::to_owned)
                .to_vec(),
            fetched_batches: fetched_batches.clone(),
        };

        assert!(ensure_batched_data(&params).await);
        assert_eq!(
            *fetched_batches.lock().unwrap(),
            vec![
                vec!["B".to_owned(), "D".to_owned(), "E".to_owned()],
                vec!["G".to_owned()],
            ]
        );
        for symbol in ["B", "D", "E", "G"] {
            assert_eq!(
                get_cached_data::<String>(&params.get_file_path(symbol))
                    .unwrap()
                    .value,
                format!("fresh-{symbol}")
            );
        }

        fs::remove_dir_all(cache_dir).unwrap();
    }
}
