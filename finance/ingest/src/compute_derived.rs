use std::{
    collections::BTreeMap, fs::{self, File}, io::{BufReader, BufWriter, Write}, path::{Path, PathBuf}, time::Instant
};

use anyhow::{Context, Result, bail};
use clap::Parser;
use futures::{StreamExt, stream};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use time::{Date, Duration};

use crate::{
    common::alpaca_api::{CashDividend, CorporateActions}, financials::{
        local_api::{
            read_company_from, read_corporate_actions_from, read_prices_from, read_sector_from, read_tradable_symbols
        }, models::{Company, Exchange, PricePoint, SECTORID_TO_NAME, Sector, SymbolMeta}
    }, ingest_utils::{common::SHORT_ISO_PARSER, is_valid_symbol}, meta_utils::get_app_data_path
};

#[derive(Parser, Debug)]
#[command(name = "ComputeDerived")]
#[command(version = "1.0")]
#[command(about = "Compute derived data using raw data fetched from various apis")]
struct Cli {
    /// Remove existing data
    #[arg(long = "remove-existing", default_value_t = false)]
    remove_existing: bool,

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

    /// Whether to remove the existing derived data before running
    remove_existing: bool,

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
        remove_existing: args.remove_existing,
        update_stocks: args.update_stocks,
        update_meta: args.update_meta,
        parallelism: resolve_parallelism(args.parallelism),
    };

    log::info!("Compute Derived - Populating...");

    if options.remove_existing {
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

    // const populateReport = {
    //     options,
    //     failedSymbols: []
    // }
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
    is_easy_to_borrow: bool,
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
    write_pretty_json(
        &derived_data_path.join("symbolToStockMeta.json"),
        &symbol_to_stock_meta,
    )?;
    write_pretty_json(&derived_data_path.join("symbols.json"), &symbols)?;

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

fn write_pretty_json<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let file =
        File::create(path).with_context(|| format!("failed to create {}", path.display()))?;
    let mut writer = BufWriter::new(file);
    serde_json::to_writer_pretty(&mut writer, value)
        .with_context(|| format!("failed to serialize {}", path.display()))?;
    writer
        .write_all(b"\n")
        .with_context(|| format!("failed to finish writing {}", path.display()))?;
    Ok(())
}

fn write_pretty_json_atomic<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let file_name = path
        .file_name()
        .with_context(|| format!("path has no file name: {}", path.display()))?
        .to_string_lossy();
    let temporary_path = path.with_file_name(format!(".{file_name}.tmp"));

    if let Err(error) = write_pretty_json(&temporary_path, value) {
        let _ = fs::remove_file(&temporary_path);
        return Err(error);
    }
    if let Err(error) = fs::rename(&temporary_path, path) {
        let _ = fs::remove_file(&temporary_path);
        return Err(error).with_context(|| {
            format!(
                "failed to replace {} with {}",
                path.display(),
                temporary_path.display()
            )
        });
    }
    Ok(())
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
                "isEasyToBorrow": true,
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
            is_easy_to_borrow: true,
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

// main()

// async function main() {

//     if (options.updateMeta) {
//         console.log(`symbolToStockMeta - Populating...`)
//         const symbolToStockMeta = await populateSymbolToStockMeta()

//         console.log(`symbols - Populating...`)
//         await fs.writeJson(
//             `${derivedDataPath}/symbols.json`,
//             Object.keys(symbolToStockMeta)
//         )

//         // // TODO: Just use copy
//         // console.log(`symbolToCefMeta - Populating...`)
//         // await fs.writeJson(
//         //     `${derivedDataPath}/symbolToCefMeta.json`,
//         //     await readSymbolToCefMeta()
//         // )
//     }
//     else {
//         console.log('Derived Data - Skipping meta...')
//     }

//     if (populateReport.failedSymbols.length) {
//         console.log('Failures: ', populateReport)
//     }
// }

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

// async function populateSymbolToStockMeta() {
//     const stockPaths = await fs.readdir(derivedStockDataPath)
//     const symbolToStockMeta: SymbolToStockMeta = {}
//     for (const stockPath of stockPaths) {
//         const stock: Stock = await fs.readJson(`${derivedStockDataPath}/${stockPath}`)

//         symbolToStockMeta[stock.symbol] = pickBy({
//             symbol: stock.symbol,
//             company: pickBy(
//                 stock.company,
//                 (v, k) => v && ['companyName', 'description'].includes(k)
//             ),
//             sector: pickBy(
//                 stock.sector,
//                 (v, k) => k.endsWith('Name')
//             ),

//             // latestQuote: stock.latestQuote,
//             // snapshot: stock.snapshot,
//             latestPoint: pickBy(
//                 stock.historicalPrices.at(-1),
//                 (v, k) => ['date', 'closeYield', 'closePrice', 'navPrice', 'navPremium', 'volume'].includes(k)
//             ),
//             cefMeta: getCefMeta(stock),
//         }, v => v != null)
//     }

//     await fs.writeJson(
//         `${derivedDataPath}/symbolToStockMeta.json`,
//         symbolToStockMeta,
//         { spaces: 4 },
//     )

//     return symbolToStockMeta

//     function getCefMeta(stock: Stock) {
//         if (!stock.cefMeta) {
//             return null
//         }
//         // TODOX: zscore
//         const v = pickBy(
//             stock.cefMeta,
//             (v, k) => [
//                 'name',
//                 'category',
//                 'strategy',
//                 'navPrice',
//                 'leverageRatio',
//                 'distributionRateOnPrice',
//                 'effectiveDurationLeverageAdjusted',
//                 'expenseRatio',
//                 'ZScore1Yr', // TODO: Rename
//             ].includes(k)
//         )

//         const returnHistory = stock?.portfolio?.fundInfo?.history;
//         if (returnHistory) {
//             v.history = returnHistory;
//         }

//         return v;
//     }
// }

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
        is_easy_to_borrow,
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
        is_easy_to_borrow,
        is_shortable,
        is_fractionable,
        company,
        sector: DerivedSector::from(sector),
        corporate_actions,
        dividends,
        dividend_metrics,
        historical_prices,
    };

    write_pretty_json_atomic(
        &derived_stock_data_path.join(format!("{symbol}.json")),
        &derived_stock,
    )
    // const [
    //     bdcMeta,
    //     cefMeta,
    //     financials,
    //     rawPortfolio,
    //     dividends,
    //     companyFacts,
    // ] = await Promise.allSettled([ // TODO: Throw if required fields missing
    //     readBdcMeta({ symbol }),
    //     readCefMeta({ symbol }),
    //     readFinancials({ symbol }),
    //     readPortfolio({ symbol }),,
    //     // TODO: Expand to all types
    //     isEquityReit ? fetchCompanyFacts({ cik: symbolMeta.cik }) : Promise.resolve()
    // ])
    // .then(results => results.map(v => v.status === 'fulfilled' ? v.value : null)) as [BdcMeta, CefMeta, Financials, NPORT_FORM_DATA, Dividend[], RawCompanyFacts]

    // const navPrices = cefMeta ? await readNavPrices({ navSymbol: cefMeta.navSymbol }).then(v => v || []) : null

    // // TODO: Is this the best way?
    // const frequencyDividends = dividends.filter(d => {
    //     const isValidFrequency = DividendFrequency[d.frequency] != null
    //     return isValidFrequency
    // })

    // const historicalPrices = frequencyDividends.length ? getHistoricalYield({
    //     dividends: frequencyDividends,
    //     historicalPrices: prices,
    //     precision: options.populatePrecision,
    // }) : prices as YieldPoint[]

    // if (navPrices?.length) {
    //     annotateNavInjectedPoints({
    //         prices: historicalPrices,
    //         navPrices,
    //         precision: options.populatePrecision,
    //     })
    // }

    // const portfolio = formatPortfolio({
    //     stats,
    //     portfolio: rawPortfolio
    // })

    // // @ts-ignore
    // // cefMeta.temp = getCefScore({
    // //     prices: historicalPrices,
    // //     cefMeta,
    // //     portfolio,
    // // })

    // const derivedStock = pickBy<Stock>({
    //     cik: symbolMeta.cik,
    //     symbol,
    //     sector,
    //     company,
    //     stats,
    //     latestQuote: quote,
    //     snapshot: getSnapshot({
    //         financials,
    //         prices: historicalPrices as YieldPoint[],
    //     }),
    //     // snapshot: undefined, // Do we want to store or compute dynamically?
    //     financials,
    //     portfolio,
    //     bdcMeta,
    //     cefMeta,
    //     dividends,
    //     splits,
    //     historicalPrices,

    //     // TODO: make this right
    //     // @ts-ignore
    //     statements: companyFacts ? createStatements(companyFacts).filter(v => v.period === 'FY') : null,
    // }, v => v != null)

    // await fs.writeJson(
    //     `${derivedStockDataPath}/${derivedStock.symbol}.json`,
    //     derivedStock,
    //     { spaces: 4 },
    // )

    // // @ts-ignore
    // return derivedStock as Stock
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
    dividends.dedup_by(|a, b| same_dividend(a, b));

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

fn same_dividend(a: &DerivedDividend, b: &DerivedDividend) -> bool {
    a.rate == b.rate
        && a.special == b.special
        && a.foreign == b.foreign
        && a.ex_date == b.ex_date
        && a.record_date == b.record_date
        && a.payable_date == b.payable_date
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

// function getCefScore({
//     prices,
//     cefMeta,
//     portfolio
// }: {
//     prices: YieldPoint[]
//     cefMeta: CefMeta
//     portfolio: Portfolio
// }) {

//     // TODO: Find spread
//     // prices, navPrices

//     // Spread score
//     //
//     // const cefType = portfolio.investments.equities

//     const discounts = prices.map(v => v.navPremium)

//     return {
//         discounts,
//         // numStandardDev from average discount over last 1y, 3y, 5y
//         //
//     }
// }

// function getSnapshot({
//     prices,
//     financials,
// }: {
//     prices: YieldPoint[]
//     financials: Stock['financials']
// }): Stock['snapshot'] {
//     const snapshot: Snapshot = {
//         // @ts-ignore
//         currentMetrics: computeCurrentMetrics({ prices }),
//         ttmMetrics: computeTtmMetrics(),
//         yearMetrics: [],
//         quarterMetrics: [],
//     }

//     // TODO: add price to ffo and price to affo
//     // if (financials.)

//     // TODO: Make timeframes actually match, just using proxy now
//     const timeAgoMetrics = [
//         { unit: '1w', timeAgo: Duration.fromObject({ weeks: 1 }), },
//         { unit: '2w', timeAgo: Duration.fromObject({ weeks: 2 }), },
//         { unit: '3w', timeAgo: Duration.fromObject({ weeks: 3 }), },
//         { unit: '1m', timeAgo: Duration.fromObject({ months: 1 }), },
//         { unit: '2m', timeAgo: Duration.fromObject({ months: 2 }), },
//         { unit: '3m', timeAgo: Duration.fromObject({ months: 3 }), },
//         { unit: '6m', timeAgo: Duration.fromObject({ months: 6 }), },
//         { unit: '1y', timeAgo: Duration.fromObject({ years: 1 }), },
//         { unit: '3y', timeAgo: Duration.fromObject({ years: 3 }), },
//         { unit: '5y', timeAgo: Duration.fromObject({ years: 5 }), },
//     ]

//     const timeAgoUpdater = createMetricUpdater()

//     let currentYearUpdater = createMetricUpdater()
//     let currentQuarterUpdater = createMetricUpdater()

//     const quartersAsc = financials?.incomeStatementsQuarterly.map(s => ({ date: DateTime.fromISO(s.date).endOf('month').startOf('day'), statement: s })) || []

//     let currentYear
//     let currentQuarter

//     let currentTimeAgoMetric = timeAgoMetrics.shift()
//     const now = DateTime.now()
//     for (let i = prices.length - 1; i > 0; i--) {
//         const point = prices[i]
//         const pointDate = DateTime.fromISO(point.date)

//         // TODO: not running on last point
//         if (currentYear == null) {
//             currentYear = pointDate.year
//         }
//         else if (currentYear !== pointDate.year) {
//             // @ts-ignore
//             snapshot.yearMetrics.unshift({
//                 year: currentYear,
//                 ...currentYearUpdater.getResult()
//             })
//             currentYear = pointDate.year
//             currentYearUpdater = createMetricUpdater()
//         }

//         // Note: PLD 2019-12-01 -> 2019-12-31. Should use end of month for end of Q
//         // Tho note that this doesn't always work, e.g. CSCO quarter ended Jan 28 instead of 31

//         // Close off quarter if point is first part of new quarter
//         // Only start quarter metric if have full quarter of data
//         // So...
//         // 1) Starting initial quarter. if currentQ is null and point === highestQ, setCurrentQ (need margin of error?)
//         // 2) when active quarter... if nextQ date is hit, close off current quarter and start next
//         // 3) How to know when minQ is finished?. Instead of checking specific dates... check 3 month range
//         // TODO: Quarter metrics shift

//         // Check if exists
//         const nextQ = quartersAsc.at(-1)
//         const shouldStartQuarterMetrics = (
//             !currentQuarter &&
//             nextQ &&
//             (pointDate.equals(nextQ.date) || nextQ.date.diff(pointDate,'days').days < 4) // TODO: make this logic more robust. Trying to handle cases where end of Q is holiday, weekend, etc
//         )
//         const isActiveQuarterFinished = (
//             currentQuarter &&
//             // TODO: Needs to check 3 month
//             (nextQ && pointDate.equals(nextQ.date) || pointDate.diff(currentQuarter, 'months').months > 3) // TODO: make this logic more robust. Trying to handle cases where end of Q is holiday, weekend, etc
//         )
//         // console.log(nextQ.date.toISO(), pointDate.toISO(), pointDate.equals(nextQ.date), nextQ.date.diff(pointDate,'days').days, shouldStartQuarterMetrics, isActiveQuarterFinished)
//         if (shouldStartQuarterMetrics) {
//             // TODO: compute price to Ffo
//             // TODO: Should use close price on last day? or median?
//             // TODO: price to ffo
//             currentQuarter = quartersAsc.pop()
//         }
//         else if (isActiveQuarterFinished) { // TODO: off by one if first point is on quarter boundary
//             // @ts-ignore
//             snapshot.quarterMetrics.unshift({
//                 quarter: currentQuarter.date.toISODate(),
//                 ...currentQuarterUpdater.getResult({
//                     financials: {
//                         incomeStatement: currentQuarter.statement,
//                     }
//                 })
//             })
//             currentQuarter = quartersAsc.pop() // will eventually produce undefined and end quarterly computation
//             currentQuarterUpdater = createMetricUpdater()
//         }

//         timeAgoUpdater.update(point)
//         currentYearUpdater.update(point)
//         currentQuarterUpdater.update(point)

//         // TODO: Should be done at end
//         const currentPointTimeAgo = now.diff(pointDate, ['months', 'years'])
//         const doesCurrentPointMeetTimeAgoCriteria = (
//             currentTimeAgoMetric &&
//             currentPointTimeAgo > currentTimeAgoMetric.timeAgo
//         )

//         if (doesCurrentPointMeetTimeAgoCriteria) {
//             snapshot[`timeAgo${currentTimeAgoMetric.unit}`] = timeAgoUpdater.getResult()
//             currentTimeAgoMetric = timeAgoMetrics.shift()
//         }
//     }

//     return snapshot

//     function computeCurrentMetrics({
//         prices,
//     }: {
//         prices: YieldPoint[]
//     }): Snapshot['currentMetrics'] {
//             // TODO: Offer opportunity to tune
//             const indicators = [
//                 { name: 'rsi7d', value: new FasterRSI(7) },
//                 { name: 'rsi14d', value: new FasterRSI(14) },
//                 { name: 'rsi30d', value: new FasterRSI(30) },
//                 { name: 'mom12d', value: new FasterMOM(12) },
//             ]

//             const numPointsToCheck = 31
//             for (const point of prices.slice(-numPointsToCheck)) {
//                 for (const indicator of indicators) {
//                     indicator.value.update(point.closePrice)
//                 }
//             }

//             // @ts-ignore
//             return Object.fromEntries(
//                 indicators.filter(indicator => indicator.value.isStable)
//                           .map(indicator => [indicator.name, round(indicator.value.getResult(), options.populatePrecision)])
//             )
//     }

//     function computeTtmMetrics(): Snapshot['ttmMetrics'] {
//         const qsl = financials?.incomeStatementsQuarterly?.length
//         if (!qsl || qsl < 4) { // Cannot compute without at least 4 quarterly statements
//             return {}
//         }

//         const last4QuarterlyStatements = financials.incomeStatementsQuarterly.slice(-4)
//         const latestQuarterlyStatement = financials.incomeStatementsQuarterly.at(-1)
//         const latestPrice = prices.at(-1)

//         // REIT specific
//         const ffoTtm = computeTtm(s => s?.supplementalItems?.ffo)
//         const affoTtm = computeTtm(s => s?.supplementalItems?.affo)

//         // TODO: Cashflow statemnet commonAndPreferredStockDividendsPaid instead?
//         const dividendsPaid = computeTtm(s => {
//             // TODO: Does it include preferred?
//             // TODO: compute using diluted also?
//             const dividendPerShare = s?.supplementalItems?.dividendPerShare
//             const sharesOutstanding = s?.supplementalItems?.ffoSharesBasic
//             if (dividendPerShare == null || sharesOutstanding == null) {
//                 return null
//             }
//             return dividendPerShare * sharesOutstanding
//         })

//         // const ffoDiluted = computeTtm(s => s?.supplementalItems?.ffoDiluted)
//         // const affoDiluted = computeTtm(s => s?.supplementalItems?.affoDiluted)

//         const { ffoSharesBasic, ffoSharesDiluted } = latestQuarterlyStatement.supplementalItems

//         const ffoPerShare = (ffoTtm != null && ffoSharesBasic != null) ? ffoTtm/ffoSharesBasic : null
//         const ffoPerShareDiluted = (ffoTtm != null && ffoSharesDiluted != null) ? ffoTtm/ffoSharesDiluted : null
//         const affoPerShare = (affoTtm != null && ffoSharesBasic != null) ? affoTtm/ffoSharesBasic : null
//         const affoPerShareDiluted = (affoTtm != null && ffoSharesDiluted != null) ? affoTtm/ffoSharesDiluted : null

//         // TODO: This is not right, using quarterly not annual
//         return pickBy<Snapshot['ttmMetrics']>({
//             ffo: ffoTtm,
//             ffoPerShare,
//             priceToFfo: (ffoPerShare != null && latestPrice.closePrice != null) ? latestPrice.closePrice / ffoPerShare : null,

//             affo: affoTtm,
//             affoPerShare,
//             priceToAffo: (affoPerShare != null && latestPrice.closePrice != null) ? latestPrice.closePrice / affoPerShare : null,

//             // ffoDiluted,
//             ffoPerShareDiluted,
//             priceToFfoDiluted: (ffoPerShareDiluted != null && latestPrice.closePrice != null) ? latestPrice.closePrice / ffoPerShareDiluted : null,

//             // affoDiluted,
//             affoPerShareDiluted,
//             priceToAffoDiluted: (affoPerShareDiluted != null && latestPrice.closePrice != null) ? latestPrice.closePrice / affoPerShareDiluted : null,

//             // TODO: Make this work for non-reits
//             dividendsPaid,
//             ffoPayoutRatio: ffoTtm != null ? dividendsPaid / ffoTtm : null,
//             // ffoPayoutRatioDiluted: dividendsPaid / ffoDiluted,
//             affoPayoutRatio: affoTtm != null ? dividendsPaid / affoTtm : null,
//             // affoPayoutRatioDiluted: dividendsPaid / affoDiluted,
//         }, Number.isFinite)

//         function computeTtm(fn: (i: IncomeStatement) => any ): any {
//             const values = last4QuarterlyStatements.map(fn)
//             return values.every(v => v != null) ? sum(values) : null
//         }
//     }
// }
// // TODO: implement and consolidate redundant calculation
// // function computePriceToFfo() {

// // }

// // function computePriceToAffo() {

// // }

// function createMetricUpdater() {
//     let numSeenPoints = 0
//     let firstSeenPoint: YieldPoint
//     let lastSeenPoint: YieldPoint
//     let sumYield = 0
//     let sumPrice = 0
//     let minPrice
//     let maxPrice
//     // TODO: Median?
//     let meanPrice
//     let minYield
//     let maxYield
//     let meanYield

//     return {
//         update,
//         getResult,
//     }

//     function getResult({
//         financials
//     }: {
//         financials?: {
//             balanceSheet?: BalanceSheet
//             cashflowStatement?: CashflowStatement
//             incomeStatement?: IncomeStatement
//         }
//     } = {}) {

//         // TODO: Always use diluted?
//         // TODO: is diluted filled in with basic when not available?
//         const { ffoPerShareDiluted, ffoPerShareBasic, affoPerShareDiluted } = financials?.incomeStatement?.supplementalItems || {}

//         // TODO: Why is this so low vs actual data?
//         // console.log(ffoPerShareBasic, ffoPerShareDiluted)
//         return pickBy<Snapshot['timeAgo1m']>({
//             minPrice,
//             maxPrice,
//             meanPrice: round(meanPrice, options.populatePrecision),
//             minYield,
//             maxYield,
//             meanYield: round(meanYield, options.populatePrecision),
//             yieldOnCost: lastSeenPoint.yieldOnCost,

//             // TODO: Use mean? Use different interface per type
//             // TODO: Problem... need to use TTM ffo, not quarterly
//             // TODO: Pull TTM financials from SA? Or compute ourself?
//             // @ts-ignore
//             priceToFfo: firstSeenPoint.closePrice / ffoPerShareDiluted,
//             priceToAffo: firstSeenPoint.closePrice / affoPerShareDiluted,
//         }, Number.isFinite)
//     }

//     function update(point: YieldPoint) {
//         numSeenPoints++

//         if (!firstSeenPoint) {
//             firstSeenPoint = point
//         }

//         minPrice = minPrice != null ? Math.min(minPrice, point.closePrice) : point.closePrice
//         maxPrice = maxPrice != null ? Math.max(maxPrice, point.closePrice) : point.closePrice
//         sumPrice += point.closePrice

//         // TODO: is marked as 0?
//         if (point.closeYield) {
//             minYield = minYield != null ? Math.min(minYield, point.closeYield) : point.closeYield
//             maxYield = maxYield != null ? Math.max(maxYield, point.closeYield) : point.closeYield
//             sumYield += point.closeYield
//         }

//         meanYield = sumYield / numSeenPoints
//         meanPrice = sumPrice / numSeenPoints
//         lastSeenPoint = point
//     }
// }

// async function getAdjustedDividends({
//     dividends,
//     splits,
//     fromCurrencyToRate,
// }: {
//     dividends: Dividend[]
//     splits: Split[]
//     fromCurrencyToRate: Record<Currency, number>
// }): Promise<Dividend[]> {
//     if (!dividends) {
//         return []
//     }
//     splits = splits || []

//     // TODO: Double check if split is on same day as dividend
//     const events = [
//         ...dividends.slice().map(d => ({
//             type: 'dividend',
//             value: d,
//         })),
//         ...splits.slice().map(s => ({
//             type: 'split',
//             value: s,
//         }))
//     ]
//     .sort((a, b) => DateTime.fromISO(b.value.exDate).toMillis() - DateTime.fromISO(a.value.exDate).toMillis())

//     const splitAdjustedDividends: Dividend[] = []

//     let splitToFactor = 1
//     for (const event of events) {
//         if (event.type === 'split') {
//             splitToFactor *= (event.value as Split).toFactor
//         }
//         else if (event.type === 'dividend') {
//             const d = event.value as Dividend

//             let normalizedFrequency = DividendFrequency.from(d.frequency)
//             if (!normalizedFrequency) {
//                 console.warn(`Dividend frequency of '${d.frequency}' is not coercible to an interpretable value!`)
//                 normalizedFrequency = 'UNKNOWN/UNSUPPORTED'
//             }

//             if (d.currency && !fromCurrencyToRate[d.currency]) {
//                 console.warn(`Exchange rate for ${d.currency} not found!`)
//             }

//             const currencyExchangeRate = fromCurrencyToRate[d.currency] || 1 // TODO: fallback to 1?
//             splitAdjustedDividends.push({
//                 amount: d.amount,
//                 currency: d.currency,
//                 // @ts-ignore
//                 frequency: normalizedFrequency,
//                 description: d.description,

//                 exDate: d.exDate,
//                 payDate: d.payDate,
//                 recordDate: d.recordDate,
//                 declareDate: d.declareDate,

//                 adjustedAmount: currencyExchangeRate * ((event.value as Dividend).amount / splitToFactor),
//                 splitAdjustmentFactor: splitToFactor,
//                 currencyExchangeRate,
//             })
//         }
//     }

//     return splitAdjustedDividends.reverse()
// }

// const issuerCategoryToType: Record<ISSUER_CATEGORY_TYPE, IssuerType> = {
//     CORP:  'Corporate',
//     UST:  'U.S. Treasury',
//     USGA:  'U.S. government agency',
//     USGSE:  'U.S. government sponsored entity',
//     MUN:  'Municipal',
//     NUSS:  'Non-U.S. sovereign',
//     PF:  'Private Fund',
//     RF:  'Registered Fund',
// }

// const assetCategoryToType: Record<ASSET_CATEGORY_TYPE, InvestmentType> = {
//     STIV: 'Short-term investment vehicle',
// 	RA: 'Repurchase Agreement',
// 	EC: 'Equity-common',
// 	EP: 'Equity-preferred',
// 	DBT: 'Debt',
// 	DCO: 'Derivative-commodity',
// 	DCR: 'Derivative-credit',
// 	DE: 'Derivative-equity',
// 	DFE: 'Derivative-foreign exchange',
// 	DIR: 'Derivative-interest rate',
// 	DO: 'Derivative-other',
// 	SN: 'Structured note',
// 	LON: 'Loan',
// 	'ABS-MBS': 'ABS-mortgage backed security',
// 	'ABS-APCP': 'ABS-asset backed commercial paper',
// 	'ABS-CBDO': 'ABS-collateralized bond/debt obligation',
// 	'ABS-O': 'ABS-other',
// 	COMM: 'Commodity', // Commodity
// 	RE: 'Real Estate', // Real estate
// }

// type InvestmentGroup = (
//     'shortTermInvestmentVehicles' |
// 	'repurchaseAgreements' |
// 	'equities' |
// 	'preferreds' |
// 	'debts' |
// 	'derivatives' |
// 	'structuredNotes' |
// 	// 'loans' |
// 	'mortgageBackedSecurities' |
// 	'assetBackedCommercialPapers' |
// 	'collateralizedDebtObligations' |
// 	'otherAssetBackedSecurities' |
// 	'commodities' |
// 	'realEstateHoldings'
// )

// const assetCategoryToGroup: Record<InvestmentType, InvestmentGroup> = {
//     'Short-term investment vehicle': 'shortTermInvestmentVehicles',
// 	'Repurchase Agreement': 'repurchaseAgreements',
// 	'Equity-common': 'equities',
// 	'Equity-preferred': 'preferreds',
// 	'Debt': 'debts',
//     'Loan': 'debts',
// 	'Derivative-commodity': 'derivatives',
// 	'Derivative-credit': 'derivatives',
// 	'Derivative-equity': 'derivatives',
// 	'Derivative-foreign exchange': 'derivatives',
// 	'Derivative-interest rate': 'derivatives',
// 	'Derivative-other': 'derivatives',
// 	'Structured note': 'structuredNotes',
// 	'ABS-mortgage backed security': 'mortgageBackedSecurities',
// 	'ABS-asset backed commercial paper': 'assetBackedCommercialPapers',
// 	'ABS-collateralized bond/debt obligation': 'collateralizedDebtObligations',
// 	'ABS-other': 'otherAssetBackedSecurities',
// 	'Commodity': 'commodities',
// 	'Real Estate': 'realEstateHoldings',
// }

// const assetGroupToExtraInfo: Partial<Record<InvestmentGroup, (v: NPORT_FORM_DATA['invstOrSecs']['invstOrSec'][number]) => any>> = {
//     equities: v => ({ numberOfShares: v.balance }),
//     preferreds: v => ({ numberOfShares: v.balance }),
//     derivatives: v => ({ numberOfContracts: v.balance }),
//     debts: v => ({
//         principal: v.balance,
//         currency: v.curCd,
//         maturityDate: v.debtSec.maturityDt,
//         couponType: v.debtSec.couponKind,
//         annualizedRate: v.debtSec.annualizedRt / 100,
//         isInDefault: v.debtSec.isDefault,
//         isPaidInArrears: v.debtSec.areIntrstPmntsInArrs,
//         isPaidInKind: v.debtSec.isPaidKind,
//     }),
// }

// function formatPortfolio({
//     portfolio,
//     stats,
// }: {
//     portfolio?: NPORT_FORM_DATA
//     stats: Stats,
// }): Portfolio {
//     if (!portfolio) {
//         return null
//     }
//     const {
//         genInfo,
//         fundInfo,
//         explntrNotes,
//         invstOrSecs,
//     } = portfolio

//     // TODO: many 0 numerics in result that shouldnt be there.
//     // e.g. PDO assetsInvested

//     // @ts-ignore
//     return pickBy<Portfolio>({
//         // TODO: This is not right. This is the date for the period, but filing can be after this date. Update
//         reportedDate: genInfo.repPdDate,
//         // TODO: get filmNumber?
//         // id: genInfo.regFileNumber,
//         seriesId: genInfo.seriesId,
//         seriesName: genInfo.seriesName,
//         seriesLei: genInfo.seriesLei,

//         cikOfRegisteredEntity: genInfo.regCik, // TODO: Should be string
//         notes: (explntrNotes?.explntrNote || []),
//         fundInfo: {
//             totalAssets: fundInfo.totAssets,
//             totalLiabilities: fundInfo.totLiabs,
//             netAssets: fundInfo.netAssets,
//             netAssetsPerShare: fundInfo.netAssets / stats.sharesOutstanding,

//             // TODO: Add context to docs about what this represents
//             borrowers: (fundInfo?.borrowers?.borrower || []).map(v => ({
//                 lei: v.lei,
//                 name: v.name,
//                 totalAmountBorrowed: v.aggrVal,
//             })),
//             history: formatHistoricalStats({ portfolio, stats }),
//         },
//         // @ts-ignore
//         investments: groupBy(
//             (invstOrSecs?.invstOrSec || [])
//                 .map((v): Partial<Investment> => {
//                     const assetType = assetCategoryToType[v.assetCat]
//                     const groupType = assetCategoryToGroup[assetType]

//                     return pickBy<Investment>({
//                         name: v.name,
//                         title: v.name !== v.title ? v.title : null,
//                         valueInUSD: v.valUSD,
//                         shareOfNavPercentage: v.pctVal / 100,

//                         // Type specific grouping
//                         ...(assetGroupToExtraInfo[groupType] || (() => ({})))(v),
//                         issuerId: v.lei,
//                         issueId: v.cusip,
//                         issuerType: issuerCategoryToType[v.issuerCat] || 'Other',
//                         issuerDescription: v?.issuerConditional?.desc,
//                         issuerCountry: v.invCountry,
//                         assetType,
//                     }, v => v != null)
//                 })
//                 .sort((a, b) => b.shareOfNavPercentage - a.shareOfNavPercentage),
//             v => assetCategoryToGroup[v.assetType],
//         )
//     }, v => v != null)
// }

// function formatHistoricalStats({
//     portfolio,
//     stats,
// }: {
//     portfolio: NPORT_FORM_DATA
//     stats: Stats,
// }): Portfolio['fundInfo']['history'] {
//     const { fundInfo } = portfolio
//     const returnInfo = portfolio?.fundInfo?.returnInfo

//     let { monthlyTotReturn } = returnInfo?.monthlyTotReturns
//     const isMultipleShareClasses = Array.isArray(monthlyTotReturn)
//     monthlyTotReturn = Array.isArray(monthlyTotReturn) ? monthlyTotReturn[0] : monthlyTotReturn

//     // returnInfo.othMon1.netRealizedGain
//     return {
//         // TODO: Support return info for each... or find way to default to investor shares
//         isMultipleShareClasses,

//         // TODOX: Figure out waht to do with this
//         oneMonthAgo: computeHistory(1),
//         twoMonthsAgo: computeHistory(2),
//         threeMonthsAgo: computeHistory(3),
//     }

//     // Dumb implementation due to nport-p spec
//     function computeHistory(month: number) {
//         const netRealizedGain = Object.values(returnInfo.monthlyReturnCats).reduce((sum, v) => sum + v[`mon${month}`].netRealizedGain, 0)
//         const netUnrealizedAppreciation = Object.values(returnInfo.monthlyReturnCats).reduce((sum, v) => sum + v[`mon${month}`].netUnrealizedAppr, 0)

//         return {
//             date: dayjs(portfolio.genInfo.repPdDate).subtract(month - 1, 'months').format('YYYY-MM-DD'),
//             // TODO: Add dates to this
//             returnPercent: monthlyTotReturn[`rtn${month}`] / 100,
//             netRealizedGain,
//             // TODOX: Won't work if share count changes (rarer for CEFs). Get shares outstanding for each month
//             netRealizedGainPerShare: netRealizedGain/stats.sharesOutstanding,
//             netUnrealizedAppreciation,
//             // TODOX: doesn't seem to work right? Doesn't match pimco UNII
//             netUnrealizedAppreciationPerShare: netUnrealizedAppreciation/stats.sharesOutstanding,
//             salesFlow: fundInfo[`mon${month}Flow`].sales,
//             reinvestmentFlow: fundInfo[`mon${month}Flow`].reinvestment,
//             redemptionFlow: fundInfo[`mon${month}Flow`].redemption,
//         }
//     }

// }
