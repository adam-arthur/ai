pub struct Stock {
    pub cik: Option<String>,
    pub symbol: String, // company: Company
                        // stats: Stats
                        // sector: Sector
                        // latestQuote: Quote
                        // snapshot: Snapshot
                        // dividends: Dividend[]
                        // splits: Split[]
                        // historicalPrices: YieldPoint[]

                        // financials?: {
                        //     balanceSheetsAnnual: BalanceSheet[],
                        //     balanceSheetsQuarterly: BalanceSheet[],
                        //     cashflowStatementsAnnual: CashflowStatement[],
                        //     cashflowStatementsQuarterly: CashflowStatement[],
                        //     incomeStatementsAnnual: IncomeStatement[],
                        //     incomeStatementsQuarterly: IncomeStatement[],
                        // }

                        // TODO;
                        // portfolio?: Portfolio

                        // Implied that it's a bdc/cef if these exist
                        // bdcMeta?: BdcMeta
                        // cefMeta?: CefMeta
}
