use serde_json::Value;
use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

fn object_field<'a>(value: &'a Value, field: &str) -> io::Result<&'a str> {
    value
        .get(field)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, format!("missing {field}")))
}

fn read_json(path: &Path) -> io::Result<Value> {
    serde_json::from_slice(&fs::read(path)?)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

fn repository_root(marketplace_path: &Path) -> io::Result<PathBuf> {
    marketplace_path
        .parent()
        .and_then(Path::parent)
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "invalid marketplace path"))
}

fn update_manifest_version(path: &Path, cachebuster: &str) -> io::Result<()> {
    let contents = fs::read_to_string(path)?;
    let manifest: Value = serde_json::from_str(&contents)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    let version = object_field(&manifest, "version")?;
    let base_version = version.split_once('+').map_or(version, |(base, _)| base);
    let old_json = serde_json::to_string(version)?;
    let new_json = serde_json::to_string(&format!("{base_version}+codex.{cachebuster}"))?;
    for separator in [": ", ":"] {
        let needle = format!("\"version\"{separator}{old_json}");
        if let Some(start) = contents.find(&needle) {
            let value_start = start + needle.len() - old_json.len();
            let mut updated = contents;
            updated.replace_range(value_start..value_start + old_json.len(), &new_json);
            return fs::write(path, updated);
        }
    }
    Err(io::Error::new(
        io::ErrorKind::InvalidData,
        "unable to locate version field",
    ))
}

fn cachebuster() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_micros()
        .to_string()
}

fn prepare(marketplace_path: &Path) -> io::Result<()> {
    let marketplace = read_json(marketplace_path)?;
    let marketplace_name = object_field(&marketplace, "name")?;
    let plugins = marketplace
        .get("plugins")
        .and_then(Value::as_array)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing plugins"))?;
    let repository_root = repository_root(marketplace_path)?;
    let cachebuster = cachebuster();

    println!("{marketplace_name}");
    for plugin in plugins {
        let plugin_name = object_field(plugin, "name")?;
        let source_path = plugin
            .get("source")
            .and_then(|source| source.get("path"))
            .and_then(Value::as_str)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing source.path"))?;
        let manifest_path = repository_root
            .join(source_path)
            .join(".codex-plugin/plugin.json");
        update_manifest_version(&manifest_path, &cachebuster)?;
        println!("{plugin_name}");
    }
    Ok(())
}

fn run() -> io::Result<()> {
    let args: Vec<_> = env::args_os().collect();
    if args.len() != 3 || args[1] != "prepare" {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "usage: plugin-installer prepare MARKETPLACE_JSON",
        ));
    }
    prepare(Path::new(&args[2]))
}

fn main() {
    if let Err(error) = run() {
        eprintln!("plugin-installer: {error}");
        std::process::exit(1);
    }
}
