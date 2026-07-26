// // Some info is missing from CEF Connect, add it in manually where applicable
// // const symbolToKnownNavSymbol = {
// //     EVT: 'XEVTX',
// // }
// const symbolToNavInfo = {
//     ASA: {
//         isInvalidNav: true, // Doesn't work with AlphaVantage
//     },
//     BANX: {
//         isInvalidNav: true,
//     },
//     EVT: {
//         navOverride: 'XEVTX',
//     },
//     MPV: {
//         isInvalidNav: true,
//     },
//     CET: {
//         isInvalidNav: true,
//     },
//     CHN: {
//         isInvalidNav: true,
//     },
//     GRF: {
//         isInvalidNav: true,
//     },
//     EIC: {
//         isInvalidNav: true,
//     },
//     CEV: {
//         isInvalidNav: true,
//     },
//     FTHY: {
//         isInvalidNav: true,
//     },
//     GUG: {
//         isInvalidNav: true,
//     },
//     CUBA: {
//         isInvalidNav: true,
//     },
//     JOF: {
//         isInvalidNav: true,
//     },
//     MEGI: {
//         isInvalidNav: true,
//     },
//     MXF: {
//         isInvalidNav: true,
//     },
//     HYB: {
//         isInvalidNav: true,
//     },
//     IRL: {
//         isInvalidNav: true,
//     },
//     JEMD: {
//         isInvalidNav: true,
//     },
//     NPF: {
//         isInvalidNav: true,
//     },
//     OXLC: {
//         isInvalidNav: true,
//     },
//     RCG: {
//         isInvalidNav: true,
//     },
//     TWN: {
//         isInvalidNav: true,
//     },
//     CEF: {
//         isInvalidNav: true,
//     },
//     VCIF: {
//         isInvalidNav: true,
//     },
// }

// // TODO: Add support for
// // https://www.cefconnect.com/api/v3/directory
// // https://www.cefconnect.com/api/v3/funds
// // https://www.cefconnect.com/api/v3/sponsors
// // https://www.cefconnect.com/api/v3/strategy

// // TODO: Only fetch raw here, move formatting to derived
pub async fn fetch_symbol_to_cef_meta() -> HashMap<String, CefMeta> {
    cef_connect::<Vec<CefMeta>>("/api/v3/dailypricing".into())
        .await
        .unwrap()
        .into_iter()
        .map(|v| (v.symbol.clone(), v))
        .collect()

    // for (const p of pricings) {
    // const navInfo = symbolToNavInfo[p.Ticker] || {}
    //         symbolToCefMeta[p.Ticker] = pickBy<CefMeta>({
    //             // sponsorId: p.SponsorId, //: 44
    //             // sponsorName: p.SponsorName, // "Franklin Templeton Investments
    //             categoryId: p.CategoryId, // TODO: Add all category as union type
    //             category: p.CategoryName, // TODO: Add all category as union type
    //             strategy: p.Strategy, // "Fixed Income - Taxable-High Yield"

    //             name: p.Name, // "Aberdeen Japan Equity Fund"
    //             symbol: p.Ticker,
    //             price: p.Price,
    //             distributionRateOnPrice: p.DistributionRatePrice / 100,
    //             updatedDate: p.LastUpdated,

    //             navSymbol: p.NavTicker,
    //             // navSymbol: navInfo.isInvalidNav ? null : (p.NavTicker || navInfo.navOverride),
    //             // navPrice: p.NAV,
    //             // distributionRateOnNav: p.DistributionRateNAV / 100,
    //             // navUpdatedDate: p.NAVPublished, // "2022-06-17T00:00:00"

    //             // isLeveraged: p.IsLeveraged,
    //             // isTermFund: p.Term,
    //             // averageDailyVolume: p.AvgDailyVolume,
    //             // inceptionDate: p.InceptionDate,

    //             // leverageRatio: p.LeverageRatioPercentage / 100,
    //             // marketCapUSDm: p.MarketCapUSDm,

    //             // uniiPerShare: p.UNIIPerShare, //: null

    //             // TODO: Compute based on nav history
    //             // discount: p.Discount,
    //             // discount52WkAvg: p.Discount52WkAvg, // -2.16368
    //             // earningsPerShare: p.EarningsPerShare, //0.4129
    //             // effectiveDurationLeverageAdjusted: p.EffDurationLevAdjusted, // 6.97
    //             // effectiveDurationNonLevAdjusted: p.EffDurationNonLevAdj, // 4.54
    //             // expenseRatio: p.ExpenseRatio / 100, //1.69
    //             // averageCoupon: p.AverageCoupon / 100, //: 5.8719
    //             // averageWeightedMaturity: p.AverageWeightedMaturity / 100, //: 6.19
    //             // averageBondPrice: p.AvgBondPrice, //: 96.7429
    //             // YTDRetOnPrice: p.YTDRetOnPrice / 100,
    //             // Yr3RetOnNav: p.Yr3RetOnNav / 100,
    //             // Yr3RetOnPrice: p.Yr3RetOnPrice / 100,
    //             // Yr5RetOnNav: p.Yr5RetOnNav / 100,
    //             // Yr5RetOnPrice: p.Yr5RetOnPrice / 100,
    //             // ZScore1Yr: p.ZScore1Yr / 100,
    //             // ZScore3M: p.ZScore3M / 100,
    //             // ZScore6M: p.ZScore6M / 100,
    //             // ZScoreDate: p.ZScoreDate,
    //     }, v=> v != null)
    // }
    // return symbolToCefMeta
}

use std::collections::HashMap;

use serde::de::DeserializeOwned;

use crate::{financials::models::CefMeta, meta_utils::YieldWatchError};

use super::HTTP;

async fn cef_connect<T>(path: String) -> Result<T, YieldWatchError>
where
    T: DeserializeOwned,
{
    let url = format!("https://www.cefconnect.com{}", path);

    let response = HTTP.get(url)
    .header("Host", "www.cefconnect.com")
    .header("Referer", "https://cefconnect.com")
    .header("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/118.0.0.0 Safari/537.36")
    .send()
    .await?;

    log::debug!("CEFConnect API - {} - \"{}\"", response.status(), path);

    let text_body = response.text().await.unwrap();

    Ok(serde_json::from_str::<T>(&text_body)
        .unwrap_or_else(|error| panic!("Failed to deserialize JSON: {} \n {}", text_body, error)))
}
