use std::{fs, path::Path};

use anyhow::{Context, Result};
use serde::Serialize;

pub fn write_json_atomic<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let file_name = path
        .file_name()
        .with_context(|| format!("path has no file name: {}", path.display()))?
        .to_string_lossy();
    let temporary_path = path.with_file_name(format!(".{file_name}.tmp"));
    let mut bytes = serde_json::to_vec_pretty(value)
        .with_context(|| format!("failed to serialize {}", path.display()))?;
    bytes.push(b'\n');

    if let Err(error) = fs::write(&temporary_path, bytes) {
        let _ = fs::remove_file(&temporary_path);
        return Err(error).with_context(|| format!("failed to write {}", temporary_path.display()));
    }
    if let Err(error) = fs::rename(&temporary_path, path) {
        let _ = fs::remove_file(&temporary_path);
        return Err(error).with_context(|| {
            format!(
                "failed to replace {} using {}",
                path.display(),
                temporary_path.display()
            )
        });
    }

    Ok(())
}
