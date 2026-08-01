use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result, bail};
use serde::Deserialize;
use strum::IntoEnumIterator;

use crate::financials::models::{Currency, ExchangeRateSnapshot};

use super::HTTP;

const ECB_EXCHANGE_RATES_URL: &str =
    "https://data-api.ecb.europa.eu/service/data/EXR/D.CAD+GBP+HKD+JPY+USD.EUR.SP00.A";

/// Fetches daily ECB reference rates. ECB observations are quoted as units of
/// each currency per euro, so the returned snapshots use EUR as their base.
pub async fn fetch_exchange_rates(start_period: Option<&str>) -> Result<Vec<ExchangeRateSnapshot>> {
    let mut request = HTTP
        .get(ECB_EXCHANGE_RATES_URL)
        .query(&[("format", "csvdata"), ("detail", "dataonly")]);

    if let Some(start_period) = start_period {
        request = request.query(&[("startPeriod", start_period)]);
    }

    let response = request
        .send()
        .await
        .context("failed to fetch ECB exchange rates")?
        .error_for_status()
        .context("ECB exchange-rate request failed")?;
    let body = response
        .bytes()
        .await
        .context("failed to read ECB exchange-rate response")?;

    parse_exchange_rates(&body)
}

#[derive(Debug, Deserialize)]
struct EcbExchangeRateRow {
    #[serde(rename = "CURRENCY")]
    currency: Currency,
    #[serde(rename = "TIME_PERIOD")]
    time_period: String,
    #[serde(rename = "OBS_VALUE")]
    value: Option<f64>,
}

fn parse_exchange_rates(csv_data: &[u8]) -> Result<Vec<ExchangeRateSnapshot>> {
    let mut rates_by_date = BTreeMap::<String, BTreeMap<Currency, f64>>::new();
    let expected_currencies = Currency::iter()
        .filter(|currency| *currency != Currency::EUR)
        .collect::<BTreeSet<_>>();

    for row in csv::Reader::from_reader(csv_data).deserialize::<EcbExchangeRateRow>() {
        let row = row.context("failed to parse ECB exchange-rate CSV")?;
        let Some(value) = row.value else {
            continue;
        };
        if !value.is_finite() || value <= 0.0 {
            bail!(
                "ECB returned invalid {} exchange rate {} for {}",
                row.currency,
                value,
                row.time_period
            );
        }

        let previous = rates_by_date
            .entry(row.time_period.clone())
            .or_default()
            .insert(row.currency, value);
        if previous.is_some() {
            bail!(
                "ECB returned duplicate {} exchange rate for {}",
                row.currency,
                row.time_period
            );
        }
    }

    if rates_by_date.is_empty() {
        bail!("ECB returned no exchange rates");
    }

    rates_by_date
        .into_iter()
        .map(|(as_of, mut rates)| {
            let actual_currencies = rates.keys().copied().collect::<BTreeSet<_>>();
            if actual_currencies != expected_currencies {
                bail!(
                    "ECB returned an incomplete currency set for {as_of}: expected {expected_currencies:?}, got {actual_currencies:?}"
                );
            }

            rates.insert(Currency::EUR, 1.0);
            Ok(ExchangeRateSnapshot {
                as_of,
                base: Currency::EUR,
                rates,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const CSV: &str = "KEY,FREQ,CURRENCY,CURRENCY_DENOM,EXR_TYPE,EXR_SUFFIX,TIME_PERIOD,OBS_VALUE\n\
EXR.D.CAD.EUR.SP00.A,D,CAD,EUR,SP00,A,2026-07-30,1.5742\n\
EXR.D.GBP.EUR.SP00.A,D,GBP,EUR,SP00,A,2026-07-30,0.86565\n\
EXR.D.HKD.EUR.SP00.A,D,HKD,EUR,SP00,A,2026-07-30,9.1123\n\
EXR.D.JPY.EUR.SP00.A,D,JPY,EUR,SP00,A,2026-07-30,172.89\n\
EXR.D.USD.EUR.SP00.A,D,USD,EUR,SP00,A,2026-07-30,1.1609\n";

    #[test]
    fn parses_a_complete_daily_snapshot() {
        let snapshots = parse_exchange_rates(CSV.as_bytes()).unwrap();

        assert_eq!(snapshots.len(), 1);
        let snapshot = &snapshots[0];
        assert_eq!(snapshot.as_of, "2026-07-30");
        assert_eq!(snapshot.base, Currency::EUR);
        assert_eq!(snapshot.rates.len(), 6);
        assert_eq!(snapshot.rates[&Currency::EUR], 1.0);
        assert_eq!(snapshot.rates[&Currency::USD], 1.1609);
    }

    #[test]
    fn calculates_cross_rates() {
        let snapshot = parse_exchange_rates(CSV.as_bytes()).unwrap().remove(0);

        let usd_to_jpy = snapshot.rate(Currency::USD, Currency::JPY).unwrap();
        let jpy_to_usd = snapshot.rate(Currency::JPY, Currency::USD).unwrap();

        assert!((usd_to_jpy - 172.89 / 1.1609).abs() < f64::EPSILON);
        assert!((usd_to_jpy * jpy_to_usd - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn rejects_incomplete_daily_snapshots() {
        let incomplete = CSV.replace(
            "EXR.D.HKD.EUR.SP00.A,D,HKD,EUR,SP00,A,2026-07-30,9.1123\n",
            "",
        );

        assert!(parse_exchange_rates(incomplete.as_bytes()).is_err());
    }

    #[test]
    fn skips_dates_without_published_rates() {
        let unpublished = "EXR.D.CAD.EUR.SP00.A,D,CAD,EUR,SP00,A,2026-07-31,\n\
EXR.D.GBP.EUR.SP00.A,D,GBP,EUR,SP00,A,2026-07-31,\n\
EXR.D.HKD.EUR.SP00.A,D,HKD,EUR,SP00,A,2026-07-31,\n\
EXR.D.JPY.EUR.SP00.A,D,JPY,EUR,SP00,A,2026-07-31,\n\
EXR.D.USD.EUR.SP00.A,D,USD,EUR,SP00,A,2026-07-31,\n";
        let csv = format!("{CSV}{unpublished}");

        let snapshots = parse_exchange_rates(csv.as_bytes()).unwrap();

        assert_eq!(snapshots.len(), 1);
        assert_eq!(snapshots[0].as_of, "2026-07-30");
    }
}
