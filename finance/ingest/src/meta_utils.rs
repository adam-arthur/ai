use std::{env, error::Error, path::PathBuf, sync::OnceLock};

static APP_DATA_PATH: OnceLock<PathBuf> = OnceLock::new();

pub fn get_app_data_path() -> &'static PathBuf {
    APP_DATA_PATH.get_or_init(|| {
        let relative_data_dir = env::var("DATA_DIR").expect("'DATA_DIR' not set!");
        return env::current_dir()
            .expect("Failed to get current working directory")
            .join(&relative_data_dir)
            .canonicalize()
            .expect(&format!(
                "'DATA_DIR' - Failed to find relative path: {}",
                &relative_data_dir
            ));
    })
}

// Define a custom error type that implements Error, Send, and Debug traits.
#[derive(Debug, Clone)]
pub struct YieldWatchError {
    message: String,
}

impl std::fmt::Display for YieldWatchError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl Error for YieldWatchError {
    // This error type is Send because it doesn't contain any non-Send types.
}

// TODO: Can we implicity box these?
impl From<std::io::Error> for YieldWatchError {
    fn from(error: std::io::Error) -> Self {
        // TODO: Type
        Self {
            message: error.to_string(),
        }
    }
}

impl From<reqwest::Error> for YieldWatchError {
    fn from(error: reqwest::Error) -> YieldWatchError {
        // TODO: Type
        Self {
            message: error.to_string(),
        }
    }
}

// pub async fn try_parse_json_response<T>(response: Response) -> Result<T, Box<dyn Error>>
// // pub async fn try_parse_json_response<T>(response: Response) -> Result<T>
// where
//     T: DeserializeOwned,
// {
//     if !response.status().is_success() {
//         // return response.error_for_status()?;
//         return Err(
//             "hello".into(), // Box::new("as".into())
//         );
//     }
//     let v = response.json::<T>().await?;
//     Ok(v)
// }

// async function tryParseJsonResponse(response: Response) {
//   if (response.status !== 200) {
//     // 429 is too many requests
//     throw new Error(
//       `${response.status} - ${response.statusText} - ${
//         new URL(response.url).pathname
//       }`
//     );
//   }

//   const rawResponse = await response.text();

//   try {
//     return JSON.parse(rawResponse);
//   } catch (e) {
//     throw new Error(`Failed to parse JSON: ${rawResponse}`);
//   }
// }
