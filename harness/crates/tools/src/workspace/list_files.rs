use std::{path::PathBuf, sync::Arc};

use itertools::Itertools;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use utils::bounded_strings;

use crate::{Tool, ToolError, TypedTool, async_trait};

use super::common::{blocking, collect_file_paths, relative_string, resolve_existing};

const DEFAULT_MAX_FILES: usize = 1_000;

pub(super) fn new(root: Arc<PathBuf>, max_output_bytes: usize) -> impl Tool {
    ListFiles { root, max_output_bytes }
}

struct ListFiles {
    root: Arc<PathBuf>,
    max_output_bytes: usize,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct ListFilesInput {
    /// Relative directory path; defaults to the workspace root.
    #[serde(default = "default_path")]
    path: String,
}

#[derive(Debug, Serialize)]
struct ListFilesOutput {
    files: Vec<String>,
    truncated: bool,
}

#[async_trait]
impl TypedTool for ListFiles {
    type Input = ListFilesInput;
    type Output = ListFilesOutput;

    fn name(&self) -> &'static str {
        "list_files"
    }

    fn description(&self) -> &'static str {
        "Recursively list files beneath a workspace directory. Paths are relative to the workspace root."
    }

    async fn invoke(&self, input: Self::Input) -> Result<Self::Output, ToolError> {
        let root = Arc::clone(&self.root);
        let max_output_bytes = self.max_output_bytes;
        blocking(move || {
            let directory = resolve_existing(&root, &input.path)?;
            if !directory.is_dir() {
                return Err(ToolError::new(format!("path is not a directory: {}", input.path)));
            }

            let mut file_paths = Vec::new();
            let traversal_truncated = collect_file_paths(&directory, &mut file_paths, DEFAULT_MAX_FILES + 1)?;
            let found = file_paths.len();
            let files = file_paths
                .into_iter()
                .sorted_unstable()
                .take(DEFAULT_MAX_FILES)
                .map(|path| relative_string(&path, &root))
                .collect::<Result<Vec<_>, _>>()?;
            let (files, output_truncated) = bounded_strings(files, max_output_bytes);
            Ok(ListFilesOutput {
                files,
                truncated: traversal_truncated || found > DEFAULT_MAX_FILES || output_truncated,
            })
        })
        .await
    }
}

fn default_path() -> String {
    ".".into()
}
