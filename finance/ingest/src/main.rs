extern crate dotenvy;

mod common;
mod compute_derived;
mod financials;
mod ingest_utils;
mod meta_utils;

use std::{collections::HashSet, env};

use crate::ingest_utils::ingest;

use compute_derived::compute_derived;
use dotenvy::dotenv;
use simple_logger::SimpleLogger;

fn load_env_vars() -> HashSet<(String, String)> {
    let pre_dot_envs = env::vars().collect::<HashSet<_>>();
    dotenv().ok();

    return env::vars()
        .collect::<HashSet<_>>()
        .difference(&pre_dot_envs)
        .cloned()
        .collect::<HashSet<_>>();
}

#[tokio::main]
async fn main() {
    SimpleLogger::new()
        .with_timestamp_format(time::macros::format_description!(
            "[day] [hour]:[minute]:[second]"
        ))
        .with_level(log::LevelFilter::Debug)
        .init()
        .unwrap();

    let project_env_vars = load_env_vars();
    log::debug!("Environment Variables: {:?}", project_env_vars);

    ingest().await;
    compute_derived().await;
}
