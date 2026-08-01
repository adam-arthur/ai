pub mod alpaca_api;
pub mod cefconnect_api;
pub mod ecb_api;
pub mod fred_api;
pub mod sec_api;
pub mod seekingalpha_api;

use std::env;

use once_cell::sync::Lazy;
use reqwest::Client;

use self::alpaca_api::{AlpacaClient, AlpacaClientArgs};

pub static ALPACA: Lazy<AlpacaClient> = Lazy::new(|| {
    AlpacaClient::new(AlpacaClientArgs {
        key: env::var("ALPACA_API_KEY").expect("env var 'ALPACA_API_KEY' is not set!"),
        secret: env::var("ALPACA_SECRET_KEY").expect("env var 'ALPACA_SECRET_KEY' is not set!"),
        paper: true,
    })
});

pub static HTTP: Lazy<Client> = Lazy::new(Client::new);
