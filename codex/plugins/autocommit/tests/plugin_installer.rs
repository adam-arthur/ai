use std::fs;
use std::process::Command;

#[test]
fn prepares_marketplace_with_rust() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path();
    let marketplace_path = root.join(".agents/plugins/marketplace.json");
    let manifest_path = root.join("plugins/autocommit/.codex-plugin/plugin.json");
    fs::create_dir_all(marketplace_path.parent().unwrap()).unwrap();
    fs::create_dir_all(manifest_path.parent().unwrap()).unwrap();
    fs::write(
        &marketplace_path,
        r#"{"name":"local","plugins":[{"name":"autocommit","source":{"path":"./plugins/autocommit"}}]}"#,
    )
    .unwrap();
    fs::write(
        &manifest_path,
        r#"{"name":"autocommit","version":"0.1.0+codex.old"}"#,
    )
    .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_plugin-installer"))
        .args(["prepare", marketplace_path.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "local\nautocommit\n"
    );
    let manifest_contents = fs::read_to_string(&manifest_path).unwrap();
    assert!(
        manifest_contents.starts_with(r#"{"name":"autocommit","version":"#),
        "{manifest_contents}"
    );
    let manifest: serde_json::Value = serde_json::from_str(&manifest_contents).unwrap();
    let version = manifest["version"].as_str().unwrap();
    assert!(version.starts_with("0.1.0+codex."), "{version}");
    assert!(!version.ends_with("old"), "{version}");
}

#[test]
fn rejects_a_malformed_marketplace() {
    let temporary = tempfile::tempdir().unwrap();
    let marketplace_path = temporary.path().join("marketplace.json");
    fs::write(&marketplace_path, "{}").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_plugin-installer"))
        .args(["prepare", marketplace_path.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!output.status.success());
}
