use std::{
    collections::BTreeMap, fs::{self, File}, io::BufReader, path::{Path, PathBuf}, time::Instant
};

use anyhow::{Context, Result, bail};
use clap::Parser;
use futures::{StreamExt, stream};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use time::{Date, Duration};

use crate::{
    common::alpaca_api::{CashDividend, CorporateActions}, file_utils::write_json_atomic, financials::{
        local_api::{
            read_company_from, read_corporate_actions_from, read_prices_from, read_sector_from, read_tradable_symbols
        }, models::{
            BorrowStatus, Company, Exchange, PricePoint, SECTORID_TO_NAME, Sector, SymbolMeta
        }
    }, ingest_utils::{common::SHORT_ISO_PARSER, is_valid_symbol}, meta_utils::get_app_data_path
};

#[derive(Parser, Debug)]
#[command(name = "ComputeDerived")]
#[command(version = "1.0")]
#[command(about = "Compute derived data using raw data fetched from various apis")]
struct Cli {
    /// Remove derived data
    #[arg(long = "remove-derived", default_value_t = false)]
    remove_derived: bool,

    /// Update stocks
    #[arg(long = "update-stocks", default_value_t = false)]
    update_stocks: bool,

    /// Update meta information
    #[arg(long = "update-meta", default_value_t = false)]
    update_meta: bool,

    /// Number of derived stocks to build concurrently (0 uses available parallelism)
    #[arg(short = 'p', long = "parallelism", default_value_t = 0)]
    parallelism: usize,
}
struct ComputeDerivedOptions {
    /// Skip any failed stocks
    bypass_failures: bool,

    /// Whether to remove the derived data before running
    remove_derived: bool,

    /// Whether to update stocks
    update_stocks: bool,

    /// Whether to update metadata
    update_meta: bool,

    /// Number of derived stocks to build concurrently
    parallelism: usize,
}

pub async fn compute_derived() {
    let args = Cli::parse();

    let derived_data_path = get_app_data_path().join("derived");
    let derived_stock_data_path = derived_data_path.join("stocks");

    log::debug!("{:#?}", args);

    let options = ComputeDerivedOptions {
        bypass_failures: true, // Weed out any stocks with missing constituent data
        remove_derived: args.remove_derived,
        update_stocks: args.update_stocks,
        update_meta: args.update_meta,
        parallelism: resolve_parallelism(args.parallelism),
    };

    log::info!("Compute Derived - Populating...");

    if options.remove_derived {
        log::info!("Compute Derived - Clearing out previous...");
        match fs::remove_dir_all(&derived_data_path) {
            Ok(_) => log::info!("Directory removed successfully"),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                log::info!("Directory did not exist, nothing to remove")
            }
            Err(_) => panic!("Error removing directory: {}", derived_data_path.display()),
        }
    } else {
        log::info!("Compute Derived - Skipping deletion...");
    }

    if options.update_stocks {
        let tradable_symbols = read_tradable_symbols()
            .unwrap_or_else(|error| panic!("Failed to read tradable symbols: {error:#}"))
            .into_iter()
            .filter(|v| is_valid_symbol(&v.symbol))
            .collect::<Vec<SymbolMeta>>();
        let report = populate_symbols(
            tradable_symbols,
            get_app_data_path().clone(),
            derived_stock_data_path.clone(),
            options.parallelism,
            options.bypass_failures,
        )
        .await
        .unwrap_or_else(|error| panic!("Failed to update derived stocks: {error:#}"));

        log::info!(
            "Derived stocks - populated {} stocks with {} failures",
            report.populated,
            report.failures.len()
        );
        for failure in report.failures {
            log::error!(
                "{} - Failed to populate derived stock: {:#}",
                failure.symbol,
                failure.cause
            );
        }
    } else {
        log::info!("Compute Derived - Skipping stocks...");
    }

    if options.update_meta {
        log::info!("symbolToStockMeta - Populating...");
        update_meta(&derived_data_path, &derived_stock_data_path)
            .unwrap_or_else(|error| panic!("Failed to update derived metadata: {error:#}"));
    } else {
        log::info!("Derived Data - Skipping meta...");
    }
}

fn resolve_parallelism(requested: usize) -> usize {
    if requested > 0 {
        return requested;
    }

    std::thread::available_parallelism()
        .map(usize::from)
        .unwrap_or(1)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DerivedStock {
    #[serde(skip_serializing_if = "Option::is_none")]
    cik: Option<String>,
    symbol: String,
    name: String,
    exchange: Exchange,
    #[serde(skip_serializing_if = "Option::is_none")]
    borrow_status: Option<BorrowStatus>,
    is_shortable: bool,
    is_fractionable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    company: Option<Company>,
    sector: DerivedSector,
    corporate_actions: Vec<CorporateActions>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    dividends: Vec<DerivedDividend>,
    #[serde(skip_serializing_if = "Option::is_none")]
    dividend_metrics: Option<DividendMetrics>,
    historical_prices: Vec<PricePoint>,
}

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
struct DerivedDividend {
    rate: f64,
    adjusted_rate: f64,
    split_adjustment_factor: f64,
    special: bool,
    foreign: bool,
    ex_date: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    record_date: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    payable_date: Option<String>,
}

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
struct DividendMetrics {
    ttm_dividend: f64,
    ttm_dividend_yield: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    forward_dividend: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    forward_dividend_yield: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    dividend_frequency: Option<DividendFrequency>,
}

#[derive(Serialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
enum DividendFrequency {
    Monthly,
    Quarterly,
    SemiAnnual,
    Annual,
}

#[serde_with::skip_serializing_none]
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DerivedSector {
    symbol: String,
    sector_name: Option<String>,
    industry_group_name: Option<String>,
    industry_name: Option<String>,
    sub_industry_name: Option<String>,
    sub_industry_gics: Option<u64>,
}

impl From<Sector> for DerivedSector {
    fn from(sector: Sector) -> Self {
        let sub_industry_gics = sector.sub_industry_gics;
        let name_for = |divisor: u64| {
            sub_industry_gics
                .and_then(|gics| u32::try_from(gics / divisor).ok())
                .and_then(|gics| SECTORID_TO_NAME.get(&gics))
                .map(|name| (*name).to_owned())
        };

        Self {
            symbol: sector.symbol,
            sector_name: name_for(1_000_000),
            industry_group_name: name_for(10_000),
            industry_name: name_for(100),
            sub_industry_name: name_for(1),
            sub_industry_gics,
        }
    }
}

struct PopulateFailure {
    symbol: String,
    cause: anyhow::Error,
}

struct PopulateReport {
    populated: usize,
    failures: Vec<PopulateFailure>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct StockMetaSource {
    symbol: String,
    company: Option<CompanyMetaSource>,
    sector: Option<SectorMetaSource>,
    #[serde(default)]
    historical_prices: Vec<LatestPoint>,
    cef_meta: Option<CefMetaSource>,
    portfolio: Option<PortfolioMetaSource>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CompanyMetaSource {
    company_name: Option<String>,
    description: Option<String>,
}

#[serde_with::skip_serializing_none]
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct SectorMetaSource {
    sector_name: Option<String>,
    industry_group_name: Option<String>,
    industry_name: Option<String>,
    sub_industry_name: Option<String>,
}

#[serde_with::skip_serializing_none]
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct LatestPoint {
    date: Option<String>,
    close_yield: Option<f64>,
    close_price: Option<f64>,
    nav_price: Option<f64>,
    nav_premium: Option<f64>,
    volume: Option<u64>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CefMetaSource {
    name: Option<String>,
    category: Option<String>,
    strategy: Option<String>,
    nav_price: Option<f64>,
    leverage_ratio: Option<f64>,
    distribution_rate_on_price: Option<f64>,
    effective_duration_leverage_adjusted: Option<f64>,
    expense_ratio: Option<f64>,
    #[serde(rename = "ZScore1Yr")]
    z_score_1yr: Option<f64>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PortfolioMetaSource {
    fund_info: Option<FundInfoMetaSource>,
}

#[derive(Deserialize)]
struct FundInfoMetaSource {
    history: Option<Value>,
}

#[serde_with::skip_serializing_none]
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct StockMeta {
    symbol: String,
    company: Option<CompanyMeta>,
    sector: Option<SectorMetaSource>,
    latest_point: Option<LatestPoint>,
    cef_meta: Option<CefMeta>,
}

#[serde_with::skip_serializing_none]
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CompanyMeta {
    company_name: Option<String>,
    description: Option<String>,
}

#[serde_with::skip_serializing_none]
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CefMeta {
    name: Option<String>,
    category: Option<String>,
    strategy: Option<String>,
    nav_price: Option<f64>,
    leverage_ratio: Option<f64>,
    distribution_rate_on_price: Option<f64>,
    effective_duration_leverage_adjusted: Option<f64>,
    expense_ratio: Option<f64>,
    #[serde(rename = "ZScore1Yr")]
    z_score_1yr: Option<f64>,
    history: Option<Value>,
}

impl From<StockMetaSource> for StockMeta {
    fn from(mut stock: StockMetaSource) -> Self {
        let company = stock.company.map(|company| CompanyMeta {
            company_name: non_empty(company.company_name),
            description: non_empty(company.description),
        });
        let history = stock
            .portfolio
            .and_then(|portfolio| portfolio.fund_info)
            .and_then(|fund_info| fund_info.history);
        let cef_meta = stock.cef_meta.map(|cef_meta| CefMeta {
            name: cef_meta.name,
            category: cef_meta.category,
            strategy: cef_meta.strategy,
            nav_price: cef_meta.nav_price,
            leverage_ratio: cef_meta.leverage_ratio,
            distribution_rate_on_price: cef_meta.distribution_rate_on_price,
            effective_duration_leverage_adjusted: cef_meta.effective_duration_leverage_adjusted,
            expense_ratio: cef_meta.expense_ratio,
            z_score_1yr: cef_meta.z_score_1yr,
            history,
        });

        Self {
            symbol: stock.symbol,
            company,
            sector: stock.sector,
            latest_point: stock.historical_prices.pop(),
            cef_meta,
        }
    }
}

fn non_empty(value: Option<String>) -> Option<String> {
    value.and_then(|value| {
        let value = value.trim();
        (!value.is_empty()).then(|| value.to_owned())
    })
}

fn update_meta(derived_data_path: &Path, derived_stock_data_path: &Path) -> Result<()> {
    let symbol_to_stock_meta = populate_symbol_to_stock_meta(derived_stock_data_path)?;
    let symbols = symbol_to_stock_meta.keys().cloned().collect::<Vec<_>>();

    fs::create_dir_all(derived_data_path).with_context(|| {
        format!(
            "failed to create derived data directory {}",
            derived_data_path.display()
        )
    })?;
    write_json_atomic(
        &derived_data_path.join("symbolToStockMeta.json"),
        &symbol_to_stock_meta,
    )?;
    write_json_atomic(&derived_data_path.join("symbols.json"), &symbols)?;

    Ok(())
}

fn populate_symbol_to_stock_meta(
    derived_stock_data_path: &Path,
) -> Result<BTreeMap<String, StockMeta>> {
    if !derived_stock_data_path.is_dir() {
        bail!(
            "derived stock directory {} does not exist; run with --update-stocks first",
            derived_stock_data_path.display()
        );
    }

    let mut stock_paths = fs::read_dir(derived_stock_data_path)
        .with_context(|| {
            format!(
                "failed to read derived stock directory {}",
                derived_stock_data_path.display()
            )
        })?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<std::io::Result<Vec<PathBuf>>>()?;
    stock_paths.retain(|path| {
        path.extension()
            .is_some_and(|extension| extension == "json")
    });
    stock_paths.sort();

    let mut symbol_to_stock_meta = BTreeMap::new();
    for stock_path in stock_paths {
        let file = File::open(&stock_path)
            .with_context(|| format!("failed to open {}", stock_path.display()))?;
        let stock = serde_json::from_reader::<_, StockMetaSource>(BufReader::new(file))
            .with_context(|| format!("failed to deserialize {}", stock_path.display()))?;
        let stock_meta = StockMeta::from(stock);
        symbol_to_stock_meta.insert(stock_meta.symbol.clone(), stock_meta);
    }

    Ok(symbol_to_stock_meta)
}

async fn populate_symbols(
    symbols: Vec<SymbolMeta>,
    data_path: PathBuf,
    derived_stock_data_path: PathBuf,
    parallelism: usize,
    bypass_failures: bool,
) -> Result<PopulateReport> {
    fs::create_dir_all(&derived_stock_data_path).with_context(|| {
        format!(
            "failed to create derived stock directory {}",
            derived_stock_data_path.display()
        )
    })?;

    let total = symbols.len();
    let mut completed = 0;
    let mut populated = 0;
    let mut failures = Vec::new();
    let started_at = Instant::now();
    let mut jobs = stream::iter(symbols.into_iter().map(|symbol_meta| {
        let data_path = data_path.clone();
        let derived_stock_data_path = derived_stock_data_path.clone();
        let symbol = symbol_meta.symbol.clone();
        async move {
            let result = tokio::task::spawn_blocking(move || {
                populate_derived_stock(&data_path, &derived_stock_data_path, symbol_meta)
            })
            .await
            .with_context(|| format!("{symbol} - derived stock worker failed"))
            .and_then(|result| result);
            (symbol, result)
        }
    }))
    .buffer_unordered(parallelism.max(1));

    while let Some((symbol, result)) = jobs.next().await {
        completed += 1;
        match result {
            Ok(()) => populated += 1,
            Err(cause) if bypass_failures => failures.push(PopulateFailure { symbol, cause }),
            Err(cause) => return Err(cause.context(format!("{symbol} - failed to populate"))),
        }

        log::info!(
            "Derived stocks - {} of {} ({:.2}%) ({:.3}s)",
            completed,
            total,
            if total == 0 {
                100.0
            } else {
                100.0 * completed as f64 / total as f64
            },
            started_at.elapsed().as_secs_f64(),
        );
    }

    Ok(PopulateReport {
        populated,
        failures,
    })
}

fn populate_derived_stock(
    data_path: &Path,
    derived_stock_data_path: &Path,
    symbol_meta: SymbolMeta,
) -> Result<()> {
    let SymbolMeta {
        symbol,
        cik,
        name,
        exchange,
        borrow_status,
        is_shortable,
        is_fractionable,
    } = symbol_meta;

    let historical_prices = read_prices_from(data_path, &symbol)?;
    let corporate_actions = read_corporate_actions_from(data_path, &symbol)?;
    let (dividends, dividend_metrics) =
        derive_dividends(&corporate_actions, historical_prices.last())?;
    let sector = read_sector_from(data_path, &symbol)?;
    let company = read_company_from(data_path, &symbol)?;
    let derived_stock = DerivedStock {
        cik,
        symbol: symbol.clone(),
        name,
        exchange,
        borrow_status,
        is_shortable,
        is_fractionable,
        company,
        sector: DerivedSector::from(sector),
        corporate_actions,
        dividends,
        dividend_metrics,
        historical_prices,
    };

    write_json_atomic(
        &derived_stock_data_path.join(format!("{symbol}.json")),
        &derived_stock,
    )
}

fn derive_dividends(
    corporate_actions: &[CorporateActions],
    latest_price: Option<&PricePoint>,
) -> Result<(Vec<DerivedDividend>, Option<DividendMetrics>)> {
    let Some(latest_price) = latest_price else {
        return Ok((Vec::new(), None));
    };
    let as_of = parse_date(&latest_price.date)?;

    let mut splits = Vec::new();
    let mut raw_dividends = Vec::new();
    for actions in corporate_actions {
        for split in &actions.forward_splits {
            splits.push((
                parse_date(&split.ex_date)?,
                checked_split_ratio(split.new_rate, split.old_rate, &split.ex_date)?,
            ));
        }
        for split in &actions.reverse_splits {
            splits.push((
                parse_date(&split.ex_date)?,
                checked_split_ratio(split.new_rate, split.old_rate, &split.ex_date)?,
            ));
        }
        raw_dividends.extend(actions.cash_dividends.iter());
    }
    splits.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.total_cmp(&b.1)));
    splits.dedup_by(|a, b| a.0 == b.0 && a.1 == b.1);

    let mut dividends = raw_dividends
        .into_iter()
        .map(|dividend| normalize_dividend(dividend, &splits, as_of))
        .collect::<Result<Vec<_>>>()?;
    dividends.sort_by(|a, b| {
        a.ex_date
            .cmp(&b.ex_date)
            .then_with(|| a.rate.total_cmp(&b.rate))
            .then_with(|| a.special.cmp(&b.special))
            .then_with(|| a.foreign.cmp(&b.foreign))
            .then_with(|| a.record_date.cmp(&b.record_date))
            .then_with(|| a.payable_date.cmp(&b.payable_date))
    });
    if dividends.is_empty() {
        return Ok((dividends, None));
    }

    let mut date_to_regular_dividend = BTreeMap::new();
    for dividend in &dividends {
        let date = parse_date(&dividend.ex_date)?;
        if !dividend.special && date <= as_of {
            *date_to_regular_dividend.entry(date).or_insert(0.0) += dividend.adjusted_rate;
        }
    }
    let regular_dividends = date_to_regular_dividend.into_iter().collect::<Vec<_>>();
    let ttm_start = as_of - Duration::days(365);
    let ttm_dividend = regular_dividends
        .iter()
        .filter(|(date, _)| *date > ttm_start)
        .map(|(_, rate)| rate)
        .sum::<f64>();

    let dividend_frequency = regular_dividends
        .last()
        .zip(infer_dividend_frequency(&regular_dividends))
        .and_then(|((latest_date, _), frequency)| {
            ((as_of - *latest_date).whole_days() <= frequency.maximum_expected_gap_days())
                .then_some(frequency)
        });
    let forward_dividend = regular_dividends
        .last()
        .zip(dividend_frequency)
        .map(|((_, rate), frequency)| rate * frequency.payments_per_year());
    let price = latest_price.close_price;
    let metrics = (price.is_finite() && price > 0.0).then(|| DividendMetrics {
        ttm_dividend,
        ttm_dividend_yield: ttm_dividend / price,
        forward_dividend,
        forward_dividend_yield: forward_dividend.map(|dividend| dividend / price),
        dividend_frequency,
    });

    Ok((dividends, metrics))
}

fn normalize_dividend(
    dividend: &CashDividend,
    splits: &[(Date, f64)],
    as_of: Date,
) -> Result<DerivedDividend> {
    let ex_date = parse_date(&dividend.ex_date)?;
    let split_adjustment_factor = splits
        .iter()
        .filter(|(split_date, _)| *split_date > ex_date && *split_date <= as_of)
        .map(|(_, ratio)| ratio)
        .product::<f64>();

    Ok(DerivedDividend {
        rate: dividend.rate,
        adjusted_rate: dividend.rate / split_adjustment_factor,
        split_adjustment_factor,
        special: dividend.special,
        foreign: dividend.foreign,
        ex_date: dividend.ex_date.clone(),
        record_date: dividend.record_date.clone(),
        payable_date: dividend.payable_date.clone(),
    })
}

fn checked_split_ratio(new_rate: f64, old_rate: f64, ex_date: &str) -> Result<f64> {
    if !new_rate.is_finite() || !old_rate.is_finite() || new_rate <= 0.0 || old_rate <= 0.0 {
        bail!("invalid split ratio {new_rate}:{old_rate} on {ex_date}");
    }
    Ok(new_rate / old_rate)
}

fn parse_date(value: &str) -> Result<Date> {
    Date::parse(value, &SHORT_ISO_PARSER).with_context(|| format!("invalid date {value}"))
}

fn infer_dividend_frequency(dividends: &[(Date, f64)]) -> Option<DividendFrequency> {
    let intervals = dividends
        .windows(2)
        .rev()
        .take(4)
        .map(|window| (window[1].0 - window[0].0).whole_days())
        .collect::<Vec<_>>();
    let interval = median(&intervals)?;

    match interval {
        20..=45 => Some(DividendFrequency::Monthly),
        70..=110 => Some(DividendFrequency::Quarterly),
        150..=220 => Some(DividendFrequency::SemiAnnual),
        300..=430 => Some(DividendFrequency::Annual),
        _ => None,
    }
}

fn median(values: &[i64]) -> Option<i64> {
    let mut values = values.to_vec();
    values.sort_unstable();
    values.get(values.len() / 2).copied()
}

impl DividendFrequency {
    fn payments_per_year(self) -> f64 {
        match self {
            Self::Monthly => 12.0,
            Self::Quarterly => 4.0,
            Self::SemiAnnual => 2.0,
            Self::Annual => 1.0,
        }
    }

    fn maximum_expected_gap_days(self) -> i64 {
        match self {
            Self::Monthly => 45,
            Self::Quarterly => 110,
            Self::SemiAnnual => 220,
            Self::Annual => 430,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use serde_json::json;

    use super::*;

    #[test]
    fn stock_meta_keeps_only_summary_fields_and_latest_point() {
        let source = serde_json::from_value::<StockMetaSource>(json!({
            "symbol": "EXAMPLE",
            "company": {
                "companyName": "Example Fund",
                "description": "",
                "website": "https://example.com"
            },
            "sector": {
                "sectorName": "Financials",
                "industryName": "Capital Markets",
                "industryGics": 402030
            },
            "historicalPrices": [
                { "date": "2024-01-01", "closePrice": 9.5, "volume": 10 },
                {
                    "date": "2024-01-02",
                    "closeYield": 0.08,
                    "closePrice": 10.0,
                    "navPrice": 11.0,
                    "navPremium": -0.09,
                    "volume": 20,
                    "openPrice": 9.75
                }
            ],
            "cefMeta": {
                "name": "Example Fund",
                "category": "Equity",
                "strategy": "Covered Call",
                "navPrice": 11.0,
                "leverageRatio": 0.2,
                "distributionRateOnPrice": 0.08,
                "effectiveDurationLeverageAdjusted": 4.2,
                "expenseRatio": 0.01,
                "ZScore1Yr": -1.5,
                "ignored": true
            },
            "portfolio": {
                "fundInfo": {
                    "history": [{ "year": 2023, "return": 0.12 }]
                }
            },
            "financials": { "ignored": true }
        }))
        .unwrap();

        assert_eq!(
            serde_json::to_value(StockMeta::from(source)).unwrap(),
            json!({
                "symbol": "EXAMPLE",
                "company": { "companyName": "Example Fund" },
                "sector": {
                    "sectorName": "Financials",
                    "industryName": "Capital Markets"
                },
                "latestPoint": {
                    "date": "2024-01-02",
                    "closeYield": 0.08,
                    "closePrice": 10.0,
                    "navPrice": 11.0,
                    "navPremium": -0.09,
                    "volume": 20
                },
                "cefMeta": {
                    "name": "Example Fund",
                    "category": "Equity",
                    "strategy": "Covered Call",
                    "navPrice": 11.0,
                    "leverageRatio": 0.2,
                    "distributionRateOnPrice": 0.08,
                    "effectiveDurationLeverageAdjusted": 4.2,
                    "expenseRatio": 0.01,
                    "ZScore1Yr": -1.5,
                    "history": [{ "year": 2023, "return": 0.12 }]
                }
            })
        );
    }

    #[test]
    fn update_meta_writes_deterministic_indexes() {
        let test_root = temporary_test_path();
        let derived_data_path = test_root.join("derived");
        let derived_stock_data_path = derived_data_path.join("stocks");
        fs::create_dir_all(&derived_stock_data_path).unwrap();
        fs::write(
            derived_stock_data_path.join("z.json"),
            r#"{"symbol":"ZZZ","historicalPrices":[]}"#,
        )
        .unwrap();
        fs::write(
            derived_stock_data_path.join("a.json"),
            r#"{"symbol":"AAA","historicalPrices":[]}"#,
        )
        .unwrap();

        update_meta(&derived_data_path, &derived_stock_data_path).unwrap();

        assert!(
            !derived_data_path
                .join(".symbolToStockMeta.json.tmp")
                .exists()
        );
        assert!(!derived_data_path.join(".symbols.json.tmp").exists());

        let symbols: Value =
            serde_json::from_reader(File::open(derived_data_path.join("symbols.json")).unwrap())
                .unwrap();
        let stock_meta: Value = serde_json::from_reader(
            File::open(derived_data_path.join("symbolToStockMeta.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(symbols, json!(["AAA", "ZZZ"]));
        assert_eq!(
            stock_meta,
            json!({
                "AAA": { "symbol": "AAA" },
                "ZZZ": { "symbol": "ZZZ" }
            })
        );

        fs::remove_dir_all(test_root).unwrap();
    }

    #[test]
    fn missing_stock_directory_has_actionable_error() {
        let missing_path = temporary_test_path().join("stocks");
        let error = match populate_symbol_to_stock_meta(&missing_path) {
            Ok(_) => panic!("missing stock directory unexpectedly succeeded"),
            Err(error) => error,
        };
        assert!(error.to_string().contains("run with --update-stocks first"));
    }

    #[test]
    fn derived_stock_is_built_from_cached_inputs() {
        let test_root = temporary_test_path();
        let derived_stock_data_path = test_root.join("derived/stocks");
        write_stock_fixtures(&test_root, "AAPL", true);
        fs::create_dir_all(&derived_stock_data_path).unwrap();

        populate_derived_stock(&test_root, &derived_stock_data_path, symbol_meta("AAPL")).unwrap();

        let stock: Value =
            serde_json::from_reader(File::open(derived_stock_data_path.join("AAPL.json")).unwrap())
                .unwrap();
        assert_eq!(
            stock,
            json!({
                "cik": "320193",
                "symbol": "AAPL",
                "name": "Apple Inc.",
                "exchange": "NASDAQ",
                "borrowStatus": "easy_to_borrow",
                "isShortable": true,
                "isFractionable": true,
                "company": {
                    "symbol": "AAPL",
                    "companyName": "Apple Inc.",
                    "exchange": "Nasdaq",
                    "industry": "Electronic Computers",
                    "website": "https://www.apple.com",
                    "description": "Apple makes consumer technology.",
                    "primarySicCode": 3571,
                    "address": "ONE APPLE PARK WAY",
                    "state": "CA",
                    "city": "CUPERTINO",
                    "zip": "95014",
                    "phone": "(408) 996-1010",
                    "stateOfIncorporation": "CA",
                    "fiscalYearEnd": "0927"
                },
                "sector": {
                    "symbol": "AAPL",
                    "sectorName": "Information Technology",
                    "industryGroupName": "Technology Hardware & Equipment",
                    "industryName": "Technology Hardware, Storage & Peripherals",
                    "subIndustryName": "Technology Hardware, Storage & Peripherals",
                    "subIndustryGics": 45202030
                },
                "corporateActions": [],
                "historicalPrices": [{
                    "date": "2024-01-02",
                    "volume": 100,
                    "closePrice": 10.0,
                    "highPrice": 11.0,
                    "lowPrice": 9.0,
                    "openPrice": 9.5
                }]
            })
        );
        assert!(!derived_stock_data_path.join(".AAPL.json.tmp").exists());

        update_meta(&test_root.join("derived"), &derived_stock_data_path).unwrap();
        let stock_meta: Value = serde_json::from_reader(
            File::open(test_root.join("derived/symbolToStockMeta.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(
            stock_meta["AAPL"]["company"],
            json!({
                "companyName": "Apple Inc.",
                "description": "Apple makes consumer technology."
            })
        );

        fs::remove_dir_all(test_root).unwrap();
    }

    #[test]
    fn dividend_metrics_distinguish_ttm_from_forward_yield() {
        let actions = serde_json::from_value::<Vec<CorporateActions>>(json!([
            {
                "date": "2024-08-15",
                "cash_dividends": [{ "rate": 0.20, "ex_date": "2024-08-15" }]
            },
            {
                "date": "2024-11-15",
                "cash_dividends": [{ "rate": 0.20, "ex_date": "2024-11-15" }]
            },
            {
                "date": "2025-02-14",
                "cash_dividends": [{ "rate": 0.20, "ex_date": "2025-02-14" }]
            },
            {
                "date": "2025-05-15",
                "cash_dividends": [
                    { "rate": 0.25, "ex_date": "2025-05-15" },
                    { "rate": 1.00, "special": true, "ex_date": "2025-05-15" }
                ]
            }
        ]))
        .unwrap();
        let price = price_point("2025-07-31", 100.0);

        let (dividends, metrics) = derive_dividends(&actions, Some(&price)).unwrap();
        let metrics = metrics.unwrap();

        assert_eq!(dividends.len(), 5);
        assert_close(metrics.ttm_dividend, 0.85);
        assert_close(metrics.ttm_dividend_yield, 0.0085);
        assert_close(metrics.forward_dividend.unwrap(), 1.0);
        assert_close(metrics.forward_dividend_yield.unwrap(), 0.01);
        assert_eq!(
            metrics.dividend_frequency,
            Some(DividendFrequency::Quarterly)
        );
    }

    #[test]
    fn dividends_are_adjusted_to_match_split_adjusted_prices() {
        let actions = serde_json::from_value::<Vec<CorporateActions>>(json!([
            {
                "date": "2020-05-08",
                "cash_dividends": [{ "rate": 0.80, "ex_date": "2020-05-08" }]
            },
            {
                "date": "2020-08-07",
                "cash_dividends": [{ "rate": 0.80, "ex_date": "2020-08-07" }]
            },
            {
                "date": "2020-08-31",
                "forward_splits": [{
                    "new_rate": 4.0,
                    "old_rate": 1.0,
                    "ex_date": "2020-08-31"
                }, {
                    "new_rate": 4.0,
                    "old_rate": 1.0,
                    "ex_date": "2020-08-31"
                }]
            }
        ]))
        .unwrap();
        let price = price_point("2020-09-01", 10.0);

        let (dividends, metrics) = derive_dividends(&actions, Some(&price)).unwrap();

        assert_eq!(dividends.len(), 2);
        for dividend in &dividends {
            assert_close(dividend.adjusted_rate, 0.20);
            assert_close(dividend.split_adjustment_factor, 4.0);
        }
        assert_close(metrics.unwrap().ttm_dividend, 0.40);
    }

    #[test]
    fn stale_dividends_are_not_presented_as_forward_income() {
        let actions = serde_json::from_value::<Vec<CorporateActions>>(json!([
            {
                "date": "2023-02-15",
                "cash_dividends": [{ "rate": 0.25, "ex_date": "2023-02-15" }]
            },
            {
                "date": "2023-05-15",
                "cash_dividends": [{ "rate": 0.25, "ex_date": "2023-05-15" }]
            }
        ]))
        .unwrap();
        let price = price_point("2025-07-31", 10.0);

        let (_, metrics) = derive_dividends(&actions, Some(&price)).unwrap();
        let metrics = metrics.unwrap();

        assert_close(metrics.ttm_dividend, 0.0);
        assert_eq!(metrics.dividend_frequency, None);
        assert_eq!(metrics.forward_dividend, None);
        assert_eq!(metrics.forward_dividend_yield, None);
    }

    fn price_point(date: &str, close_price: f64) -> PricePoint {
        PricePoint {
            date: date.to_owned(),
            volume: 100,
            close_price,
            high_price: close_price,
            low_price: close_price,
            open_price: close_price,
        }
    }

    fn assert_close(actual: f64, expected: f64) {
        assert!(
            (actual - expected).abs() < 1e-12,
            "expected {expected}, got {actual}"
        );
    }

    #[tokio::test]
    async fn populate_symbols_reports_failures_and_keeps_existing_files() {
        let test_root = temporary_test_path();
        let derived_stock_data_path = test_root.join("derived/stocks");
        write_stock_fixtures(&test_root, "AAA", true);
        write_stock_fixtures(&test_root, "BBB", false);
        fs::create_dir_all(&derived_stock_data_path).unwrap();
        let existing_path = derived_stock_data_path.join("BBB.json");
        fs::write(&existing_path, r#"{"existing":true}"#).unwrap();

        let report = populate_symbols(
            vec![symbol_meta("AAA"), symbol_meta("BBB")],
            test_root.clone(),
            derived_stock_data_path.clone(),
            2,
            true,
        )
        .await
        .unwrap();

        assert_eq!(report.populated, 1);
        assert_eq!(report.failures.len(), 1);
        assert_eq!(report.failures[0].symbol, "BBB");
        assert_eq!(
            fs::read_to_string(existing_path).unwrap(),
            r#"{"existing":true}"#
        );

        fs::remove_dir_all(test_root).unwrap();
    }

    fn symbol_meta(symbol: &str) -> SymbolMeta {
        SymbolMeta {
            symbol: symbol.to_owned(),
            cik: Some("320193".to_owned()),
            name: "Apple Inc.".to_owned(),
            exchange: Exchange::NASDAQ,
            borrow_status: Some(BorrowStatus::EasyToBorrow),
            is_shortable: true,
            is_fractionable: true,
        }
    }

    fn write_stock_fixtures(test_root: &Path, symbol: &str, include_sector: bool) {
        fs::create_dir_all(test_root.join("prices")).unwrap();
        fs::create_dir_all(test_root.join("corporateActions")).unwrap();
        fs::create_dir_all(test_root.join("sectors")).unwrap();
        fs::create_dir_all(test_root.join("companies")).unwrap();
        write_cached_value(
            &test_root.join("prices").join(format!("{symbol}.json")),
            json!([{
                "date": "2024-01-02",
                "volume": 100,
                "closePrice": 10.0,
                "highPrice": 11.0,
                "lowPrice": 9.0,
                "openPrice": 9.5
            }]),
        );
        write_cached_value(
            &test_root
                .join("corporateActions")
                .join(format!("{symbol}.json")),
            json!([]),
        );
        write_cached_value(
            &test_root.join("companies").join(format!("{symbol}.json")),
            json!({
                "symbol": symbol,
                "companyName": "Apple Inc.",
                "exchange": "Nasdaq",
                "industry": "Electronic Computers",
                "website": "https://www.apple.com",
                "description": "Apple makes consumer technology.",
                "primarySicCode": 3571,
                "address": "ONE APPLE PARK WAY",
                "state": "CA",
                "city": "CUPERTINO",
                "zip": "95014",
                "phone": "(408) 996-1010",
                "stateOfIncorporation": "CA",
                "fiscalYearEnd": "0927"
            }),
        );
        if include_sector {
            write_cached_value(
                &test_root.join("sectors").join(format!("{symbol}.json")),
                json!({ "symbol": symbol, "subIndustryGics": 45202030 }),
            );
        }
    }

    fn write_cached_value(path: &Path, value: Value) {
        fs::write(
            path,
            serde_json::to_vec_pretty(&json!({
                "meta": { "last_updated": "2024-01-02T00:00:00Z" },
                "value": value
            }))
            .unwrap(),
        )
        .unwrap();
    }

    fn temporary_test_path() -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "finance-update-meta-{}-{unique}",
            std::process::id()
        ))
    }
}
