use std::{fs, io, path::{Path, PathBuf}, sync::Arc};

use tap::Tap;
use thiserror::Error;

use crate::{ToolRegistry, ToolRegistryError};

mod common;
mod list_files;
mod read_file;
mod search_text;

use common::DEFAULT_MAX_OUTPUT_BYTES;
/// A read-only collection of tools constrained to one canonical workspace root.
#[derive(Clone, Debug)]
pub struct WorkspaceTools {
    root: Arc<PathBuf>,
    max_output_bytes: usize,
}

impl WorkspaceTools {
    pub fn new(root: impl AsRef<Path>) -> Result<Self, WorkspaceToolsError> {
        let root = fs::canonicalize(root.as_ref()).map_err(WorkspaceToolsError::Root)?;
        if !root.is_dir() {
            return Err(WorkspaceToolsError::NotDirectory(root));
        }
        Ok(Self {
            root: Arc::new(root),
            max_output_bytes: DEFAULT_MAX_OUTPUT_BYTES,
        })
    }

    pub fn with_max_output_bytes(self, max_output_bytes: usize) -> Self {
        self.tap_mut(|tools| tools.max_output_bytes = max_output_bytes)
    }

    pub fn root(&self) -> &Path {
        self.root.as_path()
    }

    pub fn register(&self, registry: &mut ToolRegistry) -> Result<(), ToolRegistryError> {
        registry.register(list_files::new(Arc::clone(&self.root), self.max_output_bytes))?;
        registry.register(read_file::new(Arc::clone(&self.root), self.max_output_bytes))?;
        registry.register(search_text::new(Arc::clone(&self.root), self.max_output_bytes))?;
        Ok(())
    }
}

#[derive(Debug, Error)]
pub enum WorkspaceToolsError {
    #[error("failed to resolve workspace root: {0}")]
    Root(#[source] io::Error),
    #[error("workspace root is not a directory: {}", .0.display())]
    NotDirectory(PathBuf),
}
