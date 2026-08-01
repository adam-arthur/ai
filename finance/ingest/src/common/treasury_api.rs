use std::{collections::BTreeMap, error::Error, fmt};

use anyhow::{Context, Result, bail};
use serde::Deserialize;
use time::{Date, macros::format_description};

use crate::financials::models::{TreasuryDuration, TreasuryRate};

use super::HTTP;

const TREASURY_RATES_BASE_URL: &str = "https://home.treasury.gov/resource-center/data-chart-center/interest-rates/daily-treasury-rates.csv";
const TREASURY_RATE_TYPE: &str = "daily_treasury_yield_curve";

/// Fetches every nominal Treasury par-yield maturity for one month in one request.
pub async fn fetch_treasury_rates(
    month: &str,
) -> Result<BTreeMap<TreasuryDuration, Vec<TreasuryRate>>> {
    match fetch_treasury_rate_period("all", month, "field_tdr_date_value_month").await {
        Ok(rates) => Ok(rates),
        Err(error) if error.is::<NoTreasuryRates>() => {
            let previous_month = previous_month(month)?;
            log::warn!(
                "Treasury returned no rates for {month}; retrying previous month {previous_month}..."
            );
            fetch_treasury_rate_period("all", &previous_month, "field_tdr_date_value_month").await
        }
        Err(error) => Err(error),
    }
}

fn previous_month(month: &str) -> Result<String> {
    if month.len() != 6 {
        bail!("invalid Treasury month '{month}'");
    }
    let year = month[..4]
        .parse::<i32>()
        .with_context(|| format!("invalid Treasury month '{month}'"))?;
    let month_number = month[4..]
        .parse::<u8>()
        .with_context(|| format!("invalid Treasury month '{month}'"))?;

    match month_number {
        1 => Ok(format!("{:04}12", year - 1)),
        2..=12 => Ok(format!("{year:04}{:02}", month_number - 1)),
        _ => bail!("invalid Treasury month '{month}'"),
    }
}

/// Bootstraps history from Treasury's annual files. Treasury's advertised all-history CSV
/// currently returns HTTP 403, while the annual downloads are stable.
pub async fn fetch_treasury_rate_history(
    start_year: i32,
    end_year: i32,
) -> Result<BTreeMap<TreasuryDuration, Vec<TreasuryRate>>> {
    let mut history = TreasuryDuration::ALL
        .into_iter()
        .map(|duration| (duration, Vec::new()))
        .collect::<BTreeMap<_, _>>();

    for year in start_year..=end_year {
        let year = year.to_string();
        let mut annual = fetch_treasury_rate_period(&year, "all", "field_tdr_date_value").await?;
        for duration in TreasuryDuration::ALL {
            history
                .get_mut(&duration)
                .expect("all Treasury durations were initialized")
                .append(
                    annual
                        .get_mut(&duration)
                        .expect("annual response has all durations"),
                );
        }
    }

    Ok(history)
}

async fn fetch_treasury_rate_period(
    year: &str,
    period: &str,
    period_parameter: &str,
) -> Result<BTreeMap<TreasuryDuration, Vec<TreasuryRate>>> {
    let url = format!("{TREASURY_RATES_BASE_URL}/{year}/{period}");
    let query = [
        ("type", TREASURY_RATE_TYPE),
        (
            period_parameter,
            if period == "all" { year } else { period },
        ),
        ("page", ""),
        ("_format", "csv"),
    ];

    let response = HTTP
        .get(url)
        .query(&query)
        .send()
        .await
        .context("failed to fetch Treasury rates")?
        .error_for_status()
        .context("Treasury-rate request failed")?;
    let body = response
        .bytes()
        .await
        .context("failed to read Treasury-rate response")?;

    parse_treasury_rates(&body)
}

#[derive(Debug, Deserialize)]
struct TreasuryCsvRow {
    #[serde(rename = "Date")]
    date: String,
    #[serde(default, rename = "1 Mo")]
    one_month: String,
    #[serde(default, rename = "3 Mo")]
    three_month: String,
    #[serde(default, rename = "6 Mo")]
    six_month: String,
    #[serde(default, rename = "1 Yr")]
    one_year: String,
    #[serde(default, rename = "2 Yr")]
    two_year: String,
    #[serde(default, rename = "3 Yr")]
    three_year: String,
    #[serde(default, rename = "5 Yr")]
    five_year: String,
    #[serde(default, rename = "7 Yr")]
    seven_year: String,
    #[serde(default, rename = "10 Yr")]
    ten_year: String,
    #[serde(default, rename = "20 Yr")]
    twenty_year: String,
    #[serde(default, rename = "30 Yr")]
    thirty_year: String,
}

#[derive(Debug)]
struct NoTreasuryRates;

impl fmt::Display for NoTreasuryRates {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("Treasury returned no rates")
    }
}

impl Error for NoTreasuryRates {}

fn parse_treasury_rates(csv_data: &[u8]) -> Result<BTreeMap<TreasuryDuration, Vec<TreasuryRate>>> {
    let mut rates = TreasuryDuration::ALL
        .into_iter()
        .map(|duration| (duration, Vec::new()))
        .collect::<BTreeMap<_, _>>();

    for row in csv::Reader::from_reader(csv_data).deserialize::<TreasuryCsvRow>() {
        let row = row.context("failed to parse Treasury-rate CSV")?;
        let date = Date::parse(&row.date, format_description!("[month]/[day]/[year]"))
            .with_context(|| format!("Treasury returned invalid date '{}'", row.date))?
            .to_string();

        for (duration, value) in [
            (TreasuryDuration::OneMonth, row.one_month),
            (TreasuryDuration::ThreeMonth, row.three_month),
            (TreasuryDuration::SixMonth, row.six_month),
            (TreasuryDuration::OneYear, row.one_year),
            (TreasuryDuration::TwoYear, row.two_year),
            (TreasuryDuration::ThreeYear, row.three_year),
            (TreasuryDuration::FiveYear, row.five_year),
            (TreasuryDuration::SevenYear, row.seven_year),
            (TreasuryDuration::TenYear, row.ten_year),
            (TreasuryDuration::TwentyYear, row.twenty_year),
            (TreasuryDuration::ThirtyYear, row.thirty_year),
        ] {
            let value = match value.trim() {
                "" | "N/A" => None,
                value => Some(value.parse::<f32>().with_context(|| {
                    format!("Treasury returned invalid {duration} rate '{value}' for {date}")
                })?),
            };
            rates
                .get_mut(&duration)
                .expect("all Treasury durations were initialized")
                .push(TreasuryRate {
                    date: date.clone(),
                    value,
                });
        }
    }

    if rates.values().all(Vec::is_empty) {
        return Err(NoTreasuryRates.into());
    }
    for duration_rates in rates.values_mut() {
        duration_rates.sort_unstable_by(|left, right| left.date.cmp(&right.date));
    }

    Ok(rates)
}

#[cfg(test)]
mod tests {
    use super::*;

    const CSV: &str = "Date,\"1 Mo\",\"1.5 Month\",\"2 Mo\",\"3 Mo\",\"4 Mo\",\"6 Mo\",\"1 Yr\",\"2 Yr\",\"3 Yr\",\"5 Yr\",\"7 Yr\",\"10 Yr\",\"20 Yr\",\"30 Yr\"\n\
07/31/2026,3.78,3.80,3.85,3.83,3.92,3.98,4.08,4.28,4.34,4.45,4.59,4.75,5.28,5.27\n\
07/30/2026,N/A,3.80,3.84,3.82,3.92,3.98,4.04,4.23,4.30,4.38,4.52,4.68,5.22,5.21\n";

    #[test]
    fn parses_all_supported_maturities_in_ascending_date_order() {
        let rates = parse_treasury_rates(CSV.as_bytes()).unwrap();

        assert_eq!(rates.len(), TreasuryDuration::ALL.len());
        assert_eq!(rates[&TreasuryDuration::OneMonth][0].date, "2026-07-30");
        assert_eq!(rates[&TreasuryDuration::OneMonth][0].value, None);
        assert_eq!(rates[&TreasuryDuration::ThirtyYear][1].value, Some(5.27));
    }

    #[test]
    fn computes_previous_month_across_year_boundary() {
        assert_eq!(previous_month("202608").unwrap(), "202607");
        assert_eq!(previous_month("202601").unwrap(), "202512");
    }

    #[test]
    fn identifies_an_empty_response() {
        let error = parse_treasury_rates(b"").unwrap_err();

        assert!(error.is::<NoTreasuryRates>());
        assert_eq!(error.to_string(), "Treasury returned no rates");
    }
}
