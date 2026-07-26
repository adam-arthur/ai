use std::{fs, path::PathBuf, sync::Arc};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use utils::bounded_str;

use crate::{Tool, ToolError, TypedTool, async_trait};

use super::common::{blocking, relative_string, resolve_existing, tool_io_error};

pub(super) fn new(root: Arc<PathBuf>, max_output_bytes: usize) -> impl Tool {
    ReadFile { root, max_output_bytes }
}

struct ReadFile {
    root: Arc<PathBuf>,
    max_output_bytes: usize,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct ReadFileInput {
    /// File path relative to the workspace root.
    path: String,
}

#[derive(Debug, Serialize)]
struct ReadFileOutput {
    path: String,
    content: String,
    bytes: usize,
    truncated: bool,
}

#[async_trait]
impl TypedTool for ReadFile {
    type Input = ReadFileInput;
    type Output = ReadFileOutput;

    fn name(&self) -> &'static str {
        "read_file"
    }

    fn description(&self) -> &'static str {
        "Read a UTF-8 text file from the workspace. Large files are truncated."
    }

    async fn invoke(&self, input: Self::Input) -> Result<Self::Output, ToolError> {
        let root = Arc::clone(&self.root);
        let max_output_bytes = self.max_output_bytes;
        blocking(move || {
            let path = resolve_existing(&root, &input.path)?;
            if !path.is_file() {
                return Err(ToolError::new(format!("path is not a file: {}", input.path)));
            }
            let bytes = fs::read(&path).map_err(tool_io_error)?;
            let total_bytes = bytes.len();
            let content = std::str::from_utf8(&bytes).map_err(|_| ToolError::new("file is not valid UTF-8 text"))?;
            let (content, truncated) = bounded_str(content, max_output_bytes);
            Ok(ReadFileOutput {
                path: relative_string(&path, &root)?,
                content: content.to_owned(),
                bytes: total_bytes,
                truncated,
            })
        })
        .await
    }
}
