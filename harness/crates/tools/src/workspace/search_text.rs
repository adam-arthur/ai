use std::{fs, path::{Path, PathBuf}, sync::Arc};

use itertools::Itertools;
use rayon::prelude::*;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use utils::ByteBudget;

use crate::{Tool, ToolError, TypedTool, async_trait};

use super::common::{blocking, collect_file_paths, relative_string, resolve_existing, tool_io_error};

const DEFAULT_MAX_SEARCH_FILES: usize = 10_000;
const DEFAULT_MAX_RESULTS: usize = 100;
const MAX_RESULTS: usize = 1_000;

pub(super) fn new(root: Arc<PathBuf>, max_output_bytes: usize) -> impl Tool {
    SearchText { root, max_output_bytes }
}

struct SearchText {
    root: Arc<PathBuf>,
    max_output_bytes: usize,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct SearchTextInput {
    /// Literal text to find.
    #[schemars(length(min = 1))]
    query: String,
    /// Relative file or directory path; defaults to the workspace root.
    #[serde(default = "default_path")]
    path: String,
    /// Maximum number of matches to return.
    #[serde(default = "default_max_results")]
    #[schemars(range(min = 1, max = 1000))]
    max_results: usize,
}

#[derive(Debug, Serialize)]
struct SearchResult {
    path: String,
    line: usize,
    text: String,
}

#[derive(Debug, Serialize)]
struct SearchTextOutput {
    results: Vec<SearchResult>,
    truncated: bool,
}

#[async_trait]
impl TypedTool for SearchText {
    type Input = SearchTextInput;
    type Output = SearchTextOutput;

    fn name(&self) -> &'static str {
        "search_text"
    }

    fn description(&self) -> &'static str {
        "Search UTF-8 workspace files for a literal text string and return matching lines."
    }

    async fn invoke(&self, input: Self::Input) -> Result<Self::Output, ToolError> {
        if input.query.is_empty() {
            return Err(ToolError::new("query cannot be empty"));
        }
        if !(1..=MAX_RESULTS).contains(&input.max_results) {
            return Err(ToolError::new(format!(
                "`max_results` must be between 1 and {MAX_RESULTS}"
            )));
        }

        let root = Arc::clone(&self.root);
        let max_output_bytes = self.max_output_bytes;
        blocking(move || {
            let target = resolve_existing(&root, &input.path)?;

            let mut files = Vec::new();
            let mut truncated = false;
            if target.is_file() {
                files.push(target);
            } else if target.is_dir() {
                truncated = collect_file_paths(&target, &mut files, DEFAULT_MAX_SEARCH_FILES)?;
                files.sort();
            } else {
                return Err(ToolError::new(format!(
                    "path is not a file or directory: {}",
                    input.path
                )));
            }

            let mut results = Vec::new();
            let mut output_budget = ByteBudget::new(max_output_bytes);
            let batch_size = rayon::current_num_threads().max(1);
            'files: for batch in files.chunks(batch_size) {
                let matches = batch
                    .par_iter()
                    .map(|file| search_file(file, &root, &input.query, input.max_results.saturating_add(1)))
                    .collect::<Result<Vec<_>, _>>()?;

                for result in matches.into_iter().flatten() {
                    let cost = result.path.len() + result.text.len();
                    if results.len() >= input.max_results || !output_budget.try_consume(cost) {
                        truncated = true;
                        break 'files;
                    }
                    results.push(result);
                }
            }
            Ok(SearchTextOutput { results, truncated })
        })
        .await
    }
}

fn default_path() -> String {
    ".".into()
}

const fn default_max_results() -> usize {
    DEFAULT_MAX_RESULTS
}

fn search_file(file: &Path, root: &Path, query: &str, limit: usize) -> Result<Vec<SearchResult>, ToolError> {
    let bytes = fs::read(file).map_err(tool_io_error)?;
    let Ok(content) = std::str::from_utf8(&bytes) else {
        return Ok(Vec::new());
    };
    let path = relative_string(file, root)?;
    Ok(content
        .lines()
        .enumerate()
        .filter(|(_, line)| line.contains(query))
        .take(limit)
        .map(|(index, line)| SearchResult {
            path: path.clone(),
            line: index + 1,
            text: line.into(),
        })
        .collect_vec())
}
