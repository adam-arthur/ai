use std::env;

use reqwest::Url;
use thiserror::Error;

const SERVER_ENV: &str = "LLAMA_SERVER";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LlamaConfig {
    server: Url,
}

impl LlamaConfig {
    pub fn new(server: impl AsRef<str>) -> Result<Self, LlamaConfigError> {
        let server = server.as_ref().trim_end_matches('/');
        let server = Url::parse(&format!("{server}/"))?;
        Ok(Self { server })
    }

    pub fn from_env() -> Result<Self, LlamaConfigError> {
        Self::from_lookup(|name| env::var(name))
    }

    pub fn server(&self) -> &Url {
        &self.server
    }

    fn from_lookup(
        mut lookup: impl FnMut(&str) -> Result<String, env::VarError>,
    ) -> Result<Self, LlamaConfigError> {
        let server = lookup(SERVER_ENV).map_err(|source| LlamaConfigError::Environment {
            name: SERVER_ENV,
            source,
        })?;
        Self::new(server)
    }
}

#[derive(Debug, Error)]
pub enum LlamaConfigError {
    #[error("failed to read `{name}`: {source}")]
    Environment {
        name: &'static str,
        #[source]
        source: env::VarError,
    },
    #[error("invalid Llama server URL: {0}")]
    InvalidUrl(#[from] url::ParseError),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_server_from_an_environment_lookup() {
        let config = LlamaConfig::from_lookup(|name| match name {
            SERVER_ENV => Ok("http://localhost:8080".to_owned()),
            _ => Err(env::VarError::NotPresent),
        })
        .unwrap();

        assert_eq!(config.server().as_str(), "http://localhost:8080/");
    }

    #[test]
    fn reports_a_missing_server_variable() {
        let error = LlamaConfig::from_lookup(|_| Err(env::VarError::NotPresent)).unwrap_err();
        assert!(matches!(
            error,
            LlamaConfigError::Environment {
                name: SERVER_ENV,
                source: env::VarError::NotPresent,
            }
        ));
    }
}
