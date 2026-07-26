use std::{fs, path::PathBuf};

use clap::Parser;
use common::Stock;

use crate::{
    financials::{
        local_api::{read_corporate_actions, read_prices, read_sector, read_tradable_symbols}, models::SymbolMeta
    }, ingest_utils::is_valid_symbol, meta_utils::get_app_data_path
};

#[derive(Parser, Debug)]
#[command(name = "ComputeDerived")]
#[command(version = "1.0")]
#[command(about = "Compute derived data using raw data fetched from various apis")]
struct Cli {
    /// Script number to execute
    #[arg(short = 's', long = "script-number", default_value_t = 1)]
    script_number: u32,

    /// Level of parallelism
    #[arg(short = 'p', long = "parallelism", default_value_t = 1)]
    parallelism: u32,

    /// Remove existing data
    #[arg(long = "remove-existing", default_value_t = false)]
    remove_existing: bool,

    /// Update stocks
    #[arg(long = "update-stocks", default_value_t = false)]
    update_stocks: bool,

    /// Update meta information
    #[arg(long = "update-meta", default_value_t = false)]
    update_meta: bool,
}
struct ComputeDerivedOptions {
    /// Skip any failed stocks
    bypass_failures: bool,

    /// Numerical precision for number outputs
    populate_precision: u8,

    /// Whether to remove the existing derived data before running
    remove_existing: bool,

    /// Whether to update stocks
    update_stocks: bool,

    /// Whether to update metadata
    update_meta: bool,
    // TODO: no need since can implement parallelism directly in rust
    // CLI Args
    // isSingleRun: _cliArgs.parallelism === 1,
    // scriptNumber: _cliArgs.scriptNumber,
    // parallelism: _cliArgs.parallelism,
}

pub async fn compute_derived() {
    let args = Cli::parse();

    let derived_data_path: String = format!("{}/derived", get_app_data_path().as_path().display());
    let derived_stock_data_path: String = format!("{}/stocks", derived_data_path);

    log::debug!("{:#?}", args);

    let options = ComputeDerivedOptions {
        bypass_failures: true, // Weed out any stocks with missing constituent data
        populate_precision: 4,

        remove_existing: args.remove_existing,
        update_stocks: args.update_stocks,
        update_meta: args.update_meta,
    };

    log::info!("Compute Derived - Populating...");

    if options.remove_existing {
        log::info!("Compute Derived - Clearing out previous...");
        match fs::remove_dir_all(&derived_data_path) {
            Ok(_) => log::info!("Directory removed successfully"),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                log::info!("Directory did not exist, nothing to remove")
            }
            Err(_) => panic!("Error removing directory: {}", &derived_data_path),
        }
    } else {
        log::info!("Compute Derived - Skipping deletion...");
    }

    if options.update_stocks {
        let tradable_symbols = read_tradable_symbols()
            .await
            .into_iter()
            .filter(|v| is_valid_symbol(&v.symbol))
            .collect::<Vec<SymbolMeta>>();

        // TODO:
        // const exchangeRates: ExchangeRate[] = await readExchangeRates()

        //         // ADAM HERE https://www.sec.gov/cgi-bcin/viewer?action=view&cik=0001025378&accession_number=0001025378-22-000041&xbrl_type=v#
        //         // TODO: parallelize, not using full CPU
        //         // num batches from cli

        // await populateSymbols({
        //     symbols: tradableSymbols,
        //     fromCurrencyToRate: exchangeRates.reduce((fromCurrencyToRate, rate) => {
        //         fromCurrencyToRate[rate.from] = rate.rate
        //         return fromCurrencyToRate
        //     }, {}) as Record<Currency, number>
        // })
    } else {
        log::info!("Compute Derived - Skipping stocks...");
    }

    // const populateReport = {
    //     options,
    //     failedSymbols: []
    // }
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

async fn populate_symbols(symbols: Vec<SymbolMeta>) {
    //     {
    //     symbols,
    //     fromCurrencyToRate,
    // }: {
    //     symbols: SymbolMeta[]
    //     fromCurrencyToRate: Record<Currency, number>
    // }) {
    let i = 0;
    for symbol_meta in symbols {
        // const startTime = Date.now()
        log::info!("{} - Populating derived stock...", symbol_meta.symbol);

        let _ = populate_derived_stock(
            symbol_meta, // {
                         // symbolMeta,
                         // fromCurrencyToRate,
                         // }
        )
        .await;
        // try {
        //     await populateDerivedStock({
        //         symbolMeta,
        //         fromCurrencyToRate,
        //     })
        // }
        // catch (e: any) {
        //     console.error(e)
        //     console.error(`${symbolMeta.symbol} - Failed to process, ${options.bypassFailures ? 'skipping' : 'canceling'}...`)

        //     populateReport.failedSymbols.push({ symbol: symbolMeta.symbol, cause: e.message })

        //     if (!options.bypassFailures) {
        //         console.log('\n')
        //         return
        //     }
        // }
        // finally {
        //     const elapsedTime = Date.now() - startTime
        //     console.log(`Stock ${i + 1} of ${symbols.length} (${Number(100 * ((i + 1) / symbols.length)).toFixed(2)}%) (${Number(elapsedTime/1000).toFixed(3)}s)`)
        //     console.log()
        //     i++
        // }
    }
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

async fn populate_derived_stock(
    symbol_meta: SymbolMeta, //     {
                             //     symbolMeta,
                             //     fromCurrencyToRate,
                             // }: {
                             //     symbolMeta: SymbolMeta
                             //     fromCurrencyToRate: Record<Currency, number>
                             // }
) -> Stock {
    let SymbolMeta { symbol, .. } = symbol_meta;

    let prices = read_prices(&symbol);
    // TODO: Dividends are not adjusted
    let corporate_actions = read_corporate_actions(&symbol);
    let sector = read_sector(&symbol);

    // get_adjusted_dividends({
    //     dividends: rawDividends,
    //     splits,
    //     fromCurrencyToRate,
    // })

    Stock {
        cik: None,
        symbol: "TODO".into(),
    }
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
