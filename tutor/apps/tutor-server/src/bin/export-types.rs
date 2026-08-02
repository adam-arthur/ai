use std::path::PathBuf;

use anyhow::{Context as _, Result};

fn main() -> Result<()> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let output = root.join("apps/tutor/app/generated/api.ts");
    tutor_api::export_type_script(&output)
        .with_context(|| format!("failed to write {}", output.display()))?;
    println!("generated {}", output.display());
    Ok(())
}
