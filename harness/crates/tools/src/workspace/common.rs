use std::{fs, io, path::{Path, PathBuf}};

use ignore::WalkBuilder;

use crate::ToolError;

pub(super) const DEFAULT_MAX_OUTPUT_BYTES: usize = 64 * 1024;

const SKIPPED_DIRECTORIES: [&str; 5] = [".git", ".hg", ".svn", "node_modules", "target"];

pub(super) async fn blocking<T>(
    operation: impl FnOnce() -> Result<T, ToolError> + Send + 'static,
) -> Result<T, ToolError>
where
    T: Send + 'static,
{
    tokio::task::spawn_blocking(operation)
        .await
        .map_err(|error| ToolError::new(format!("blocking tool operation failed: {error}")))?
}

pub(super) fn resolve_existing(root: &Path, requested: &str) -> Result<PathBuf, ToolError> {
    let requested_path = Path::new(requested);
    if requested_path.is_absolute() {
        return Err(ToolError::new("path must be relative to the workspace root"));
    }
    let resolved = fs::canonicalize(root.join(requested_path)).map_err(tool_io_error)?;
    if !resolved.starts_with(root) {
        return Err(ToolError::new("path escapes the workspace root"));
    }
    Ok(resolved)
}

pub(super) fn collect_file_paths(directory: &Path, files: &mut Vec<PathBuf>, limit: usize) -> Result<bool, ToolError> {
    let mut builder = WalkBuilder::new(directory);
    builder
        .hidden(false)
        .follow_links(false)
        .require_git(false)
        .sort_by_file_path(Path::cmp)
        .filter_entry(|entry| {
            entry.depth() == 0
                || !entry.file_type().is_some_and(|file_type| file_type.is_dir())
                || !should_skip_directory(entry.file_name())
        });

    for entry in builder.build().skip(1) {
        let entry = entry.map_err(|error| ToolError::new(error.to_string()))?;
        if entry.file_type().is_some_and(|file_type| file_type.is_file()) {
            files.push(entry.into_path());
            if files.len() >= limit {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

pub(super) fn relative_string(path: &Path, root: &Path) -> Result<String, ToolError> {
    path.strip_prefix(root)
        .map(|path| path.to_string_lossy().into_owned())
        .map_err(|_| ToolError::new("resolved path is outside the workspace root"))
}

pub(super) fn tool_io_error(error: io::Error) -> ToolError {
    ToolError::new(error.to_string())
}

fn should_skip_directory(name: &std::ffi::OsStr) -> bool {
    name.to_str().is_some_and(|name| SKIPPED_DIRECTORIES.contains(&name))
}
