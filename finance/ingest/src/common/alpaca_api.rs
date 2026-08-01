use std::{
    collections::{BTreeMap, HashMap, HashSet}, env, num::NonZeroU32, time::Duration
};

use governor::{Quota, RateLimiter};
use itertools::{Itertools, chain};
use once_cell::sync::Lazy;
use reqwest::StatusCode;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use strum_macros::{Display, EnumString};
use tokio::time::sleep;

use crate::{
    common::ALPACA,
    financials::models::{BorrowStatus, Exchange, PricePoint},
    // ADAMTODO: Circular dependency, fix
    ingest_utils::common::{SHORT_ISO_PARSER, parse_short_iso, str_to_short_iso, to_end_of_day},
};

use super::HTTP;

const DEFAULT_ALPACA_REQUESTS_PER_MINUTE: u32 = 9_000;
const ALPACA_MAX_RETRIES: u32 = 5;

static ALPACA_API_RATE_LIMITER: Lazy<
    RateLimiter<
        governor::state::NotKeyed,
        governor::state::InMemoryState,
        governor::clock::DefaultClock,
    >,
> = Lazy::new(|| {
    let requests_per_minute = env::var("ALPACA_REQUESTS_PER_MINUTE")
        .ok()
        .and_then(|value| value.parse::<u32>().ok())
        .and_then(NonZeroU32::new)
        .unwrap_or(NonZeroU32::new(DEFAULT_ALPACA_REQUESTS_PER_MINUTE).unwrap());
    RateLimiter::direct(Quota::per_minute(requests_per_minute))
});

pub struct AlpacaClientArgs {
    pub key: String,
    pub secret: String,
    pub paper: bool,
}

pub struct AlpacaClient {
    key: String,
    secret: String,
    #[allow(dead_code)]
    is_paper: bool,
    urls: AlpacaUrls,
}

#[allow(dead_code)]
struct AlpacaUrls {
    account: String,
    market_data: String,

    account_socket: String,
    market_data_socket: String,
    news_socket: String,
}

impl AlpacaClient {
    pub fn new(args: AlpacaClientArgs) -> Self {
        Self {
            key: args.key,
            secret: args.secret,
            is_paper: args.paper,
            urls: AlpacaUrls {
                account: if args.paper {
                    "https://paper-api.alpaca.markets".into()
                } else {
                    "https://api.alpaca.markets".into()
                },
                market_data: "https://data.alpaca.markets".into(),

                account_socket: if args.paper {
                    "wss://paper-api.alpaca.markets/stream".into()
                } else {
                    "wss://api.alpaca.markets/stream".into()
                },
                market_data_socket: "wss://stream.data.alpaca.markets/v2/sip".into(),
                news_socket: "wss://stream.data.alpaca.markets/v1beta1/news".into(),
            },
        }
    }

    pub async fn fetch_from_alpaca<T>(&self, url: &str, query_params: &Vec<(String, String)>) -> T
    where
        T: DeserializeOwned,
    {
        for attempt in 0..=ALPACA_MAX_RETRIES {
            ALPACA_API_RATE_LIMITER.until_ready().await;
            let response = HTTP
                .get(url)
                .query(query_params)
                .header("Apca-Api-Key-Id", &self.key)
                .header("Apca-Api-Secret-Key", &self.secret)
                .send()
                .await;

            let response = match response {
                Ok(response) => response,
                Err(error) if attempt < ALPACA_MAX_RETRIES => {
                    let delay = retry_delay(attempt, None);
                    log::warn!(
                        "Alpaca request failed (attempt {}): {}; retrying in {:.1}s",
                        attempt + 1,
                        error,
                        delay.as_secs_f32()
                    );
                    sleep(delay).await;
                    continue;
                }
                Err(error) => panic!("Alpaca request failed after retries: {error}"),
            };

            let status = response.status();
            log::debug!(
                "Alpaca API - {} - \"{}\" Params: {:?}",
                status.as_str(),
                url,
                query_params,
            );

            if status.is_success() {
                return response.json::<T>().await.unwrap_or_else(|error| {
                    panic!("Alpaca: Failed to parse JSON response: {error}")
                });
            }

            let retry_after = response
                .headers()
                .get(reqwest::header::RETRY_AFTER)
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.parse::<u64>().ok())
                .map(Duration::from_secs);
            let body = response
                .text()
                .await
                .unwrap_or_else(|error| format!("failed to read response body: {error}"));
            let is_retryable = status == StatusCode::TOO_MANY_REQUESTS || status.is_server_error();
            if is_retryable && attempt < ALPACA_MAX_RETRIES {
                let delay = retry_delay(attempt, retry_after);
                log::warn!(
                    "Alpaca returned {} (attempt {}); retrying in {:.1}s",
                    status,
                    attempt + 1,
                    delay.as_secs_f32()
                );
                sleep(delay).await;
                continue;
            }

            panic!("Alpaca Status: {status} - Content: {body}");
        }

        unreachable!("Alpaca retry loop always returns or panics")
    }

    pub async fn get_assets(&self, args: &GetAssetsArgs) -> Vec<Asset> {
        self.fetch_from_alpaca::<Vec<Asset>>(
            &format!("{}/v2/assets", self.urls.account),
            &args.to_query_params(),
        )
        .await
    }

    async fn get_bars(&self, args: &GetBarsArgs) -> PageOfBars {
        self.fetch_from_alpaca::<PageOfBars>(
            &format!("{}/v2/stocks/{}/bars", self.urls.market_data, args.symbol),
            &args.to_query_params(),
        )
        .await
    }

    async fn get_corporate_actions(
        &self,
        args: GetCorporateActionsArgs,
    ) -> AlpacaCorporateActionsResponse {
        self.fetch_from_alpaca::<AlpacaCorporateActionsResponse>(
            &format!("{}/v1beta1/corporate-actions", self.urls.market_data),
            &args.to_query_params(),
        )
        .await
    }

    // TODO: Add support (need to handle image response types)
    // async fn get_logo(
    //     &self,
    //     symbol: String,
    // ) -> AlpacaCorporateActionsResponse {
    //     self.fetch_from_alpaca::<AlpacaCorporateActionsResponse>(
    //         &format!("{}/v1beta1/corporate-actions", self.urls.market_data),
    //         &args.to_query_params(),
    //     )
    //     .await
    // }
}

fn retry_delay(attempt: u32, retry_after: Option<Duration>) -> Duration {
    retry_after.unwrap_or_else(|| Duration::from_secs(2u64.pow(attempt.min(5))))
}

#[derive(Deserialize)]
struct AlpacaCorporateActionsResponse {
    corporate_actions: AlpacaCorporateActions,
    next_page_token: Option<String>, // Token that can be used to query the next page.
}

#[derive(Serialize, Deserialize, Debug)]
pub struct CorporateActions {
    pub date: String,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cash_dividends: Vec<CashDividend>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cash_mergers: Vec<CashMerger>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub forward_splits: Vec<ForwardSplit>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub name_changes: Vec<NameChange>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub redemptions: Vec<Redemption>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reverse_splits: Vec<ReverseSplit>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rights_distributions: Vec<RightsDistribution>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub spin_offs: Vec<SpinOff>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub stock_and_cash_mergers: Vec<StockAndCashMerger>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub stock_dividends: Vec<StockDividend>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub stock_mergers: Vec<StockMerger>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unit_splits: Vec<UnitSplit>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub worthless_removals: Vec<WorthlessRemoval>,
}

fn is_false(b: &bool) -> bool {
    !*b
}

#[derive(Serialize, Deserialize, Debug)]
pub struct CashDividend {
    pub rate: f64,
    #[serde(default, skip_serializing_if = "is_false")]
    pub special: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub foreign: bool, // ADAMTODO: How to get?
    pub ex_date: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub record_date: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payable_date: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub due_bill_on_date: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub due_bill_off_date: Option<String>,
}
impl From<AlpacaCashDividend> for CashDividend {
    fn from(source: AlpacaCashDividend) -> Self {
        CashDividend {
            rate: source.rate,
            special: source.special,
            foreign: source.foreign, // ADAMTODO: How to get?
            ex_date: source.ex_date,
            record_date: source.record_date,
            payable_date: source.payable_date,
            due_bill_on_date: source.due_bill_on_date,
            due_bill_off_date: source.due_bill_off_date,
        }
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct CashMerger {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub acquirer_symbol: Option<String>,
    pub acquiree_symbol: String,
    pub rate: f64,
    pub effective_date: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payable_date: Option<String>,
}
impl From<AlpacaCashMerger> for CashMerger {
    fn from(source: AlpacaCashMerger) -> Self {
        CashMerger {
            acquirer_symbol: source.acquirer_symbol,
            acquiree_symbol: source.acquiree_symbol,
            rate: source.rate,
            effective_date: source.effective_date,
            payable_date: source.payable_date,
        }
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ReverseSplit {
    pub new_rate: f64,
    pub old_rate: f64,
    pub ex_date: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub record_date: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payable_date: Option<String>,
}
impl From<AlpacaReverseSplit> for ReverseSplit {
    fn from(source: AlpacaReverseSplit) -> Self {
        ReverseSplit {
            new_rate: source.new_rate,
            old_rate: source.old_rate,
            ex_date: source.ex_date,
            record_date: source.record_date,
            payable_date: source.payable_date,
        }
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ForwardSplit {
    pub new_rate: f64,
    pub old_rate: f64,
    pub ex_date: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub record_date: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payable_date: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub due_bill_redemption_date: Option<String>,
}
impl From<AlpacaForwardSplit> for ForwardSplit {
    fn from(source: AlpacaForwardSplit) -> Self {
        ForwardSplit {
            new_rate: source.new_rate,
            old_rate: source.old_rate,
            ex_date: source.ex_date,
            record_date: source.record_date,
            payable_date: source.payable_date,
            due_bill_redemption_date: source.due_bill_redemption_date,
        }
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct UnitSplit {
    pub old_symbol: String,
    pub old_rate: f64,
    pub new_symbol: String,
    pub new_rate: f64,
    pub alternate_symbol: String,
    pub alternate_rate: f64,
    pub effective_date: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payable_date: Option<String>,
}
impl From<AlpacaUnitSplit> for UnitSplit {
    fn from(source: AlpacaUnitSplit) -> Self {
        UnitSplit {
            old_symbol: source.old_symbol,
            old_rate: source.old_rate,
            new_symbol: source.new_symbol,
            new_rate: source.new_rate,
            alternate_symbol: source.alternate_symbol,
            alternate_rate: source.alternate_rate,
            effective_date: source.effective_date,
            payable_date: source.payable_date,
        }
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct StockDividend {
    pub rate: f64,
    pub ex_date: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub record_date: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payable_date: Option<String>,
}
impl From<AlpacaStockDividend> for StockDividend {
    fn from(source: AlpacaStockDividend) -> Self {
        StockDividend {
            rate: source.rate,
            ex_date: source.ex_date,
            record_date: source.record_date,
            payable_date: source.payable_date,
        }
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct SpinOff {
    pub source_symbol: String,
    pub source_rate: f64,
    pub new_symbol: String,
    pub new_rate: f64,
    pub ex_date: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub record_date: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payable_date: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub due_bill_redemption_date: Option<String>,
}
impl From<AlpacaSpinOff> for SpinOff {
    fn from(source: AlpacaSpinOff) -> Self {
        SpinOff {
            source_symbol: source.source_symbol,
            source_rate: source.source_rate,
            new_symbol: source.new_symbol,
            new_rate: source.new_rate,
            ex_date: source.ex_date,
            record_date: source.record_date,
            payable_date: source.payable_date,
            due_bill_redemption_date: source.due_bill_redemption_date,
        }
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct StockMerger {
    pub acquirer_symbol: String,
    pub acquirer_rate: f64,
    pub acquiree_symbol: String,
    pub acquiree_rate: f64,
    pub effective_date: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payable_date: Option<String>,
}
impl From<AlpacaStockMerger> for StockMerger {
    fn from(source: AlpacaStockMerger) -> Self {
        StockMerger {
            acquirer_symbol: source.acquirer_symbol,
            acquirer_rate: source.acquirer_rate,
            acquiree_symbol: source.acquiree_symbol,
            acquiree_rate: source.acquiree_rate,
            effective_date: source.effective_date,
            payable_date: source.payable_date,
        }
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct StockAndCashMerger {
    pub acquirer_symbol: String,
    pub acquirer_rate: f64,
    pub acquiree_symbol: String,
    pub acquiree_rate: f64,
    pub cash_rate: f64,
    pub effective_date: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payable_date: Option<String>,
}
impl From<AlpacaStockAndCashMerger> for StockAndCashMerger {
    fn from(source: AlpacaStockAndCashMerger) -> Self {
        StockAndCashMerger {
            acquirer_symbol: source.acquirer_symbol,
            acquirer_rate: source.acquirer_rate,
            acquiree_symbol: source.acquiree_symbol,
            acquiree_rate: source.acquiree_rate,
            cash_rate: source.cash_rate,
            effective_date: source.effective_date,
            payable_date: source.payable_date,
        }
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Redemption {
    pub rate: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payable_date: Option<String>,
}
impl From<AlpacaRedemption> for Redemption {
    fn from(source: AlpacaRedemption) -> Self {
        Redemption {
            rate: source.rate,
            payable_date: source.payable_date,
        }
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct NameChange {
    pub old_symbol: String,
    pub new_symbol: String,
}
impl From<AlpacaNameChange> for NameChange {
    fn from(source: AlpacaNameChange) -> Self {
        NameChange {
            old_symbol: source.old_symbol,
            new_symbol: source.new_symbol,
        }
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct WorthlessRemoval {
    pub symbol: String,
}
impl From<AlpacaWorthlessRemoval> for WorthlessRemoval {
    fn from(source: AlpacaWorthlessRemoval) -> Self {
        WorthlessRemoval {
            symbol: source.symbol,
        }
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct RightsDistribution {
    pub source_symbol: String,
    pub new_symbol: String,
    pub rate: f64,
    pub ex_date: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub record_date: Option<String>,
    pub payable_date: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expiration_date: Option<String>,
}

impl From<AlpacaRightsDistribution> for RightsDistribution {
    fn from(source: AlpacaRightsDistribution) -> Self {
        RightsDistribution {
            source_symbol: source.source_symbol,
            new_symbol: source.new_symbol,
            rate: source.rate,
            ex_date: source.ex_date,
            record_date: source.record_date,
            payable_date: source.payable_date,
            expiration_date: source.expiration_date,
        }
    }
}

#[derive(Deserialize, Debug, Default)]
pub struct AlpacaCorporateActions {
    #[serde(default)]
    pub reverse_splits: Vec<AlpacaReverseSplit>,
    #[serde(default)]
    pub forward_splits: Vec<AlpacaForwardSplit>,
    #[serde(default)]
    pub unit_splits: Vec<AlpacaUnitSplit>,
    #[serde(default)]
    pub stock_dividends: Vec<AlpacaStockDividend>,
    #[serde(default)]
    pub cash_dividends: Vec<AlpacaCashDividend>,
    #[serde(default)]
    pub spin_offs: Vec<AlpacaSpinOff>,
    #[serde(default)]
    pub cash_mergers: Vec<AlpacaCashMerger>,
    #[serde(default)]
    pub stock_mergers: Vec<AlpacaStockMerger>,
    #[serde(default)]
    pub stock_and_cash_mergers: Vec<AlpacaStockAndCashMerger>,
    #[serde(default)]
    pub redemptions: Vec<AlpacaRedemption>,
    #[serde(default)]
    pub name_changes: Vec<AlpacaNameChange>,
    #[serde(default)]
    pub worthless_removals: Vec<AlpacaWorthlessRemoval>,
    #[serde(default)]
    pub rights_distributions: Vec<AlpacaRightsDistribution>,
}

#[derive(Deserialize, Debug)]
pub struct AlpacaReverseSplit {
    #[serde(rename = "symbol")]
    pub _symbol: String,
    pub new_rate: f64,
    pub old_rate: f64,
    pub process_date: String,
    pub ex_date: String,
    pub record_date: Option<String>,
    pub payable_date: Option<String>,
}

#[derive(Deserialize, Debug)]
pub struct AlpacaForwardSplit {
    #[serde(rename = "symbol")]
    pub _symbol: String,
    pub new_rate: f64,
    pub old_rate: f64,
    pub process_date: String,
    pub ex_date: String,
    pub record_date: Option<String>,
    pub payable_date: Option<String>,
    pub due_bill_redemption_date: Option<String>,
}

#[derive(Deserialize, Debug)]
pub struct AlpacaUnitSplit {
    pub old_symbol: String,
    pub old_rate: f64,
    pub new_symbol: String,
    pub new_rate: f64,
    pub alternate_symbol: String,
    pub alternate_rate: f64,
    pub process_date: String,
    pub effective_date: String,
    pub payable_date: Option<String>,
}

#[derive(Deserialize, Debug)]
pub struct AlpacaStockDividend {
    #[serde(rename = "symbol")]
    pub _symbol: String,
    pub rate: f64,
    pub process_date: String,
    pub ex_date: String,
    pub record_date: Option<String>,
    pub payable_date: Option<String>,
}

#[derive(Deserialize, Debug)]
pub struct AlpacaCashDividend {
    #[serde(rename = "symbol")]
    pub _symbol: String,
    pub rate: f64,
    pub special: bool,
    pub foreign: bool,
    pub process_date: String,
    pub ex_date: String,
    pub record_date: Option<String>,
    pub payable_date: Option<String>,
    pub due_bill_on_date: Option<String>,
    pub due_bill_off_date: Option<String>,
}

#[derive(Deserialize, Debug)]
pub struct AlpacaSpinOff {
    pub source_symbol: String,
    pub source_rate: f64,
    pub new_symbol: String,
    pub new_rate: f64,
    pub process_date: String,
    pub ex_date: String,
    pub record_date: Option<String>,
    pub payable_date: Option<String>,
    pub due_bill_redemption_date: Option<String>,
}

#[derive(Deserialize, Debug)]
pub struct AlpacaCashMerger {
    pub acquirer_symbol: Option<String>,
    pub acquiree_symbol: String,
    pub rate: f64,
    pub process_date: String,
    pub effective_date: String,
    pub payable_date: Option<String>,
}

#[derive(Deserialize, Debug)]
pub struct AlpacaStockMerger {
    pub acquirer_symbol: String,
    pub acquirer_rate: f64,
    pub acquiree_symbol: String,
    pub acquiree_rate: f64,
    pub process_date: String,
    pub effective_date: String,
    pub payable_date: Option<String>,
}

#[derive(Deserialize, Debug)]
pub struct AlpacaStockAndCashMerger {
    pub acquirer_symbol: String,
    pub acquirer_rate: f64,
    pub acquiree_symbol: String,
    pub acquiree_rate: f64,
    pub cash_rate: f64,
    pub process_date: String,
    pub effective_date: String,
    pub payable_date: Option<String>,
}

#[derive(Deserialize, Debug)]
pub struct AlpacaRedemption {
    #[serde(rename = "symbol")]
    pub _symbol: String,
    pub rate: f64,
    pub process_date: String,
    pub payable_date: Option<String>,
}

#[derive(Deserialize, Debug)]
pub struct AlpacaNameChange {
    pub old_symbol: String,
    pub new_symbol: String,
    pub process_date: String,
}

#[derive(Deserialize, Debug)]
pub struct AlpacaWorthlessRemoval {
    pub symbol: String,
    pub process_date: String,
}

#[derive(Deserialize, Debug)]
pub struct AlpacaRightsDistribution {
    pub source_symbol: String,
    pub new_symbol: String,
    pub rate: f64,
    pub process_date: String,
    pub ex_date: String,
    pub record_date: Option<String>,
    pub payable_date: String,
    pub expiration_date: Option<String>,
}

#[derive(Debug, EnumString, Display)]
#[strum(serialize_all = "snake_case")]
pub enum CorporateActionType {
    ReverseSplit,
    ForwardSplit,
    UnitSplit,
    CashDividend,
    StockDividend,
    SpinOff,
    CashMerger,
    StockMerger,
    StockAndCashMerger,
    Redemption,
    NameChange,
    WorthlessRemoval,
    RightsDistribution,
}

#[derive(Serialize, Deserialize, Debug)]
pub enum AssetStatus {
    #[serde(rename = "active")]
    Active,
    #[serde(rename = "inactive")]
    Inactive,
}
impl AssetStatus {
    fn get_value(&self) -> String {
        match self {
            AssetStatus::Active => "active".into(),
            AssetStatus::Inactive => "inactive".into(),
        }
    }
}

pub struct GetAssetsArgs {
    pub status: Option<AssetStatus>,
    pub asset_class: Option<String>,
}
impl GetAssetsArgs {
    pub fn to_query_params(&self) -> Vec<(String, String)> {
        let mut params: Vec<(String, String)> = Vec::with_capacity(2);
        if let Some(status) = &self.status {
            params.push(("status".into(), status.get_value()))
        }
        if let Some(asset_class) = &self.asset_class {
            params.push(("asset_class".into(), asset_class.to_owned()));
        }
        params
    }
}

pub struct GetBarsArgs {
    symbol: String,
    start: String,
    end: Option<String>,
    limit: Option<u32>,         // Can go up to 10_000
    timeframe: String,          // '1Sec' | '1Min' | '1Hour' | '1Day'
    adjustment: Option<String>, // 'all' | 'dividend' | 'raw' | 'split'
    page_token: Option<String>,
}

#[derive(Default, Debug)]
pub struct GetCorporateActionsArgs {
    symbols: Vec<String>,
    corporate_action_types: Vec<CorporateActionType>,
    start: String,
    end: Option<String>,
    limit: Option<u32>, // Can go up to 10_000
    page_token: Option<String>,
}

impl GetCorporateActionsArgs {
    pub fn to_query_params(&self) -> Vec<(String, String)> {
        let mut params: Vec<(String, String)> = Vec::with_capacity(7);

        params.push(("symbols".into(), self.symbols.join(",")));
        params.push((
            "types".into(),
            self.corporate_action_types
                .iter()
                .map(|action| action.to_string())
                .collect::<Vec<String>>()
                .join(","),
        ));
        params.push(("start".into(), self.start.clone()));

        if let Some(end) = &self.end {
            params.push(("end".into(), end.clone()))
        }
        if let Some(limit) = &self.limit {
            params.push(("limit".into(), limit.to_string()))
        }

        params.push(("sort".into(), "asc".into()));

        if let Some(page_token) = &self.page_token {
            params.push(("page_token".into(), page_token.clone()))
        }

        params
    }
}

impl GetBarsArgs {
    pub fn to_query_params(&self) -> Vec<(String, String)> {
        let mut params: Vec<(String, String)> = Vec::with_capacity(7);
        params.push(("start".into(), self.start.clone()));

        if let Some(end) = &self.end {
            params.push(("end".into(), end.into()))
        }
        if let Some(limit) = &self.limit {
            params.push(("limit".into(), limit.to_string()))
        }

        params.push(("timeframe".into(), self.timeframe.clone()));

        if let Some(adjustment) = &self.adjustment {
            params.push(("adjustment".into(), adjustment.clone()))
        }
        params.push(("feed".into(), "sip".into()));
        if let Some(page_token) = &self.page_token {
            params.push(("page_token".into(), page_token.clone()))
        }
        params
    }
}

/// The assets API serves as the master list of assets available for trade and data
/// consumption from Alpaca. Assets are sorted by asset class, exchange and symbol. Some
/// assets are only available for data consumption via Polygon, and are not tradable with
/// Alpaca. These assets will be marked with the flag tradable=false.
#[derive(Serialize, Deserialize, Debug)]
pub struct Asset {
    /// Asset ID
    pub id: String,

    /// "us_equity"
    pub class: String,

    /// AMEX, ARCA, BATS, NYSE, NASDAQ or NYSEARCA
    pub exchange: Exchange,

    /// Asset symbol
    pub symbol: String,

    /// "Apple"
    pub name: String,

    /// active or inactive
    pub status: AssetStatus,

    /// Asset is tradable on Alpaca or not
    pub tradable: bool,

    /// Asset is marginable or not
    pub marginable: bool,

    /// Asset is shortable or not
    pub shortable: bool,

    /// Current Alpaca borrow classification. This may be absent for assets where borrow
    /// availability is not applicable or unavailable.
    pub borrow_status: Option<BorrowStatus>,

    /// Asset is fractionable or not.
    pub fractionable: bool,
}

#[derive(Serialize, Deserialize, Debug)]
struct AlpacaBar {
    /** Timestamp in RFC-3339 format with nanosecond precision. */
    pub t: String,
    /** Open price. */
    pub o: f64,
    /** High price. */
    pub h: f64,
    /** Low price. */
    pub l: f64,
    /** Close price. */
    pub c: f64,
    /** Volume. */
    pub v: u64,

    /* What is this? */
    pub n: u64,

    /* Volume weighted price */
    pub vw: f64,
}

#[derive(Serialize, Deserialize, Debug)]
struct PageOfBars {
    bars: Option<Vec<AlpacaBar>>,
    symbol: String,
    next_page_token: Option<String>, // Token that can be used to query the next page.
}

pub async fn fetch_historical_prices(
    symbol: String,     // AAPL
    start_date: String, // 2011-08-27
    end_date: String,   // 2021-08-27
) -> Vec<PricePoint> {
    let end = Some(to_end_of_day(&end_date));

    // TODO: Can define one unique memory region across alll calls?
    let mut points: Vec<PricePoint> = vec![];
    let mut next_page_token: Option<String> = None;
    let mut has_more_pages = true;

    while has_more_pages {
        // TODO: actually validate, security
        let bars = ALPACA
            .get_bars(&GetBarsArgs {
                symbol: symbol.clone(),
                start: start_date.clone(),
                end: end.clone(),
                limit: Some(5000),
                timeframe: "1Day".into(),
                page_token: next_page_token,
                adjustment: Some("split".into()), // Adjust for stock splits
            })
            .await;

        match bars.bars {
            Some(bar_values) if !bar_values.is_empty() => {
                next_page_token = bars.next_page_token;
                has_more_pages = next_page_token.is_some();

                points.extend(bar_values.iter().map(|v| PricePoint {
                    date: str_to_short_iso(&v.t),
                    close_price: v.c,
                    high_price: v.h,
                    low_price: v.l,
                    open_price: v.o,
                    volume: v.v,
                }));
            }
            _ => return points,
        }
    }

    points
}

// https://data.alpaca.markets/v1beta1/corporate-actions
// start YYYY-MM-DD
pub async fn fetch_corporate_actions(symbol: &str, start: &str) -> Vec<CorporateActions> {
    // let end = Some(to_end_of_day(&end_date));
    let mut corporate_actions: Vec<CorporateActions> = Vec::new();
    let mut outer_next_page_token: Option<String> = None;

    loop {
        let AlpacaCorporateActionsResponse {
            corporate_actions:
                AlpacaCorporateActions {
                    cash_dividends,
                    cash_mergers,
                    forward_splits,
                    name_changes,
                    redemptions,
                    reverse_splits,
                    rights_distributions,
                    spin_offs,
                    stock_and_cash_mergers,
                    stock_dividends,
                    stock_mergers,
                    unit_splits,
                    worthless_removals,
                },
            next_page_token,
        } = ALPACA
            .get_corporate_actions(GetCorporateActionsArgs {
                symbols: vec![symbol.into()],
                corporate_action_types: vec![
                    CorporateActionType::CashDividend,
                    CorporateActionType::CashMerger,
                    CorporateActionType::ForwardSplit,
                    CorporateActionType::NameChange,
                    CorporateActionType::Redemption,
                    CorporateActionType::ReverseSplit,
                    CorporateActionType::RightsDistribution,
                    CorporateActionType::SpinOff,
                    CorporateActionType::StockAndCashMerger,
                    CorporateActionType::StockDividend,
                    CorporateActionType::StockMerger,
                    CorporateActionType::UnitSplit,
                    CorporateActionType::WorthlessRemoval,
                ],
                start: start.into(),
                limit: Some(1_000),
                page_token: outer_next_page_token,
                ..Default::default()
            })
            .await;

        let process_dates: Vec<String> = chain!(
            cash_dividends.iter().map(|v| &v.process_date),
            cash_mergers.iter().map(|v| &v.process_date),
            forward_splits.iter().map(|v| &v.process_date),
            name_changes.iter().map(|v| &v.process_date),
            redemptions.iter().map(|v| &v.process_date),
            reverse_splits.iter().map(|v| &v.process_date),
            rights_distributions.iter().map(|v| &v.process_date),
            spin_offs.iter().map(|v| &v.process_date),
            stock_and_cash_mergers.iter().map(|v| &v.process_date),
            stock_dividends.iter().map(|v| &v.process_date),
            stock_mergers.iter().map(|v| &v.process_date),
            unit_splits.iter().map(|v| &v.process_date),
            worthless_removals.iter().map(|v| &v.process_date),
        )
        .collect::<HashSet<&String>>()
        .into_iter()
        .map(|v| parse_short_iso(v))
        .sorted()
        .map(|v| v.format(&SHORT_ISO_PARSER).expect("Failed to format date"))
        .collect();

        // ADAMTODO: DRY
        let mut date_to_cashdividends = cash_dividends
            .into_iter()
            .into_group_map_by(|v| v.process_date.clone());
        let mut date_to_cashmergers = cash_mergers
            .into_iter()
            .into_group_map_by(|v| v.process_date.clone());
        let mut date_to_forwardsplits = forward_splits
            .into_iter()
            .into_group_map_by(|v| v.process_date.clone());
        let mut date_to_namechanges = name_changes
            .into_iter()
            .into_group_map_by(|v| v.process_date.clone());
        let mut date_to_redemptions = redemptions
            .into_iter()
            .into_group_map_by(|v| v.process_date.clone());
        let mut date_to_reversesplits = reverse_splits
            .into_iter()
            .into_group_map_by(|v| v.process_date.clone());
        let mut date_to_rightsdistributions = rights_distributions
            .into_iter()
            .into_group_map_by(|v| v.process_date.clone());
        let mut date_to_spinoffs = spin_offs
            .into_iter()
            .into_group_map_by(|v| v.process_date.clone());
        let mut date_to_stockandcashmergers = stock_and_cash_mergers
            .into_iter()
            .into_group_map_by(|v| v.process_date.clone());
        let mut date_to_stockdividends = stock_dividends
            .into_iter()
            .into_group_map_by(|v| v.process_date.clone());
        let mut date_to_stockmergers = stock_mergers
            .into_iter()
            .into_group_map_by(|v| v.process_date.clone());
        let mut date_to_unitsplits = unit_splits
            .into_iter()
            .into_group_map_by(|v| v.process_date.clone());
        let mut date_to_worthlessremovals = worthless_removals
            .into_iter()
            .into_group_map_by(|v| v.process_date.clone());

        // TODO: Will date format be same?
        // NOTE: Assuming dates are ordered (as AlpacaApi implies)
        for date in process_dates {
            corporate_actions.push(CorporateActions {
                cash_dividends: remove_into(&mut date_to_cashdividends, &date),
                cash_mergers: remove_into(&mut date_to_cashmergers, &date),
                forward_splits: remove_into(&mut date_to_forwardsplits, &date),
                name_changes: remove_into(&mut date_to_namechanges, &date),
                redemptions: remove_into(&mut date_to_redemptions, &date),
                reverse_splits: remove_into(&mut date_to_reversesplits, &date),
                rights_distributions: remove_into(&mut date_to_rightsdistributions, &date),
                spin_offs: remove_into(&mut date_to_spinoffs, &date),
                stock_and_cash_mergers: remove_into(&mut date_to_stockandcashmergers, &date),
                stock_dividends: remove_into(&mut date_to_stockdividends, &date),
                stock_mergers: remove_into(&mut date_to_stockmergers, &date),
                unit_splits: remove_into(&mut date_to_unitsplits, &date),
                worthless_removals: remove_into(&mut date_to_worthlessremovals, &date),
                date,
            })
        }

        outer_next_page_token = next_page_token;
        if outer_next_page_token.is_none() {
            break;
        }
    }

    canonicalize_corporate_actions(corporate_actions)
}

pub(crate) fn merge_corporate_actions(
    mut cached: Vec<CorporateActions>,
    fetched: Vec<CorporateActions>,
) -> Vec<CorporateActions> {
    let Some(first_fetched_date) = fetched.first().map(|actions| actions.date.as_str()) else {
        return cached;
    };

    let boundary = cached.partition_point(|actions| actions.date.as_str() < first_fetched_date);
    cached.truncate(boundary);
    cached.extend(fetched);
    cached
}

fn canonicalize_corporate_actions(
    corporate_actions: Vec<CorporateActions>,
) -> Vec<CorporateActions> {
    let mut actions_by_date = BTreeMap::<String, CorporateActions>::new();

    for actions in corporate_actions {
        match actions_by_date.entry(actions.date.clone()) {
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert(actions);
            }
            std::collections::btree_map::Entry::Occupied(mut entry) => {
                append_corporate_actions(entry.get_mut(), actions);
            }
        }
    }

    for actions in actions_by_date.values_mut() {
        deduplicate_regular_cash_dividends(&mut actions.cash_dividends);
    }

    actions_by_date.into_values().collect()
}

fn append_corporate_actions(target: &mut CorporateActions, mut source: CorporateActions) {
    macro_rules! append_actions {
        ($($field:ident),+ $(,)?) => {
            $(target.$field.append(&mut source.$field);)+
        };
    }

    append_actions!(
        cash_dividends,
        cash_mergers,
        forward_splits,
        name_changes,
        redemptions,
        reverse_splits,
        rights_distributions,
        spin_offs,
        stock_and_cash_mergers,
        stock_dividends,
        stock_mergers,
        unit_splits,
        worthless_removals,
    );
}

fn deduplicate_regular_cash_dividends(dividends: &mut Vec<CashDividend>) {
    let mut regular_dividends = HashSet::new();
    dividends.retain(|dividend| {
        dividend.special
            || regular_dividends.insert((
                dividend.rate.to_bits(),
                dividend.foreign,
                dividend.ex_date.clone(),
                dividend.record_date.clone(),
                dividend.payable_date.clone(),
                dividend.due_bill_on_date.clone(),
                dividend.due_bill_off_date.clone(),
            ))
    });
}

fn remove_into<V, T>(map: &mut HashMap<String, Vec<V>>, key: &String) -> Vec<T>
where
    V: Into<T>,
{
    map.remove(key)
        .unwrap_or_default()
        .into_iter()
        .map(Into::into)
        .collect()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn bar_requests_always_use_sip() {
        let params = GetBarsArgs {
            symbol: "AAPL".into(),
            start: "2026-07-01T00:00:00Z".into(),
            end: None,
            limit: Some(100),
            timeframe: "1Day".into(),
            adjustment: Some("split".into()),
            page_token: None,
        }
        .to_query_params();

        assert!(params.contains(&("feed".into(), "sip".into())));
    }

    #[test]
    fn canonicalization_deduplicates_only_regular_cash_dividends() {
        let actions = serde_json::from_value::<Vec<CorporateActions>>(json!([
            {
                "date": "2025-05-15",
                "cash_dividends": [
                    { "rate": 0.25, "ex_date": "2025-05-15" },
                    { "rate": 0.25, "ex_date": "2025-05-15" },
                    {
                        "rate": 0.25,
                        "ex_date": "2025-05-15",
                        "due_bill_on_date": "2025-05-16"
                    },
                    { "rate": 1.00, "special": true, "ex_date": "2025-05-15" }
                ]
            },
            {
                "date": "2025-05-15",
                "cash_dividends": [
                    { "rate": 1.00, "special": true, "ex_date": "2025-05-15" },
                    { "rate": 2.00, "special": true, "ex_date": "2025-05-15" }
                ]
            }
        ]))
        .unwrap();

        let canonical = canonicalize_corporate_actions(actions);

        assert_eq!(canonical.len(), 1);
        let dividends = &canonical[0].cash_dividends;
        assert_eq!(dividends.len(), 5);
        assert_eq!(
            dividends.iter().filter(|dividend| dividend.special).count(),
            3
        );
        assert_eq!(
            dividends
                .iter()
                .filter(|dividend| !dividend.special)
                .count(),
            2
        );
    }

    #[test]
    fn merge_replaces_the_refetched_date_and_keeps_earlier_history() {
        let cached = serde_json::from_value::<Vec<CorporateActions>>(json!([
            {
                "date": "2025-05-14",
                "cash_dividends": [{ "rate": 0.20, "ex_date": "2025-05-14" }]
            },
            {
                "date": "2025-05-15",
                "cash_dividends": [{ "rate": 0.25, "ex_date": "2025-05-15" }]
            }
        ]))
        .unwrap();
        let fetched = serde_json::from_value::<Vec<CorporateActions>>(json!([
            {
                "date": "2025-05-15",
                "cash_dividends": [{ "rate": 0.30, "ex_date": "2025-05-15" }]
            },
            {
                "date": "2025-05-16",
                "cash_dividends": [{ "rate": 0.40, "ex_date": "2025-05-16" }]
            }
        ]))
        .unwrap();

        let merged = merge_corporate_actions(cached, fetched);

        assert_eq!(
            merged
                .iter()
                .map(|actions| actions.date.as_str())
                .collect::<Vec<_>>(),
            vec!["2025-05-14", "2025-05-15", "2025-05-16"]
        );
        assert_eq!(merged[0].cash_dividends[0].rate, 0.20);
        assert_eq!(merged[1].cash_dividends[0].rate, 0.30);
    }
}
