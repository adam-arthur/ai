use std::fs;

use serde_json::json;
use tempfile::tempdir;
use tools::{ToolRegistry, WorkspaceTools};

#[tokio::test]
async fn workspace_tools_list_read_and_search_fixture_files() {
    let directory = tempdir().unwrap();
    fs::create_dir(directory.path().join("src")).unwrap();
    fs::create_dir(directory.path().join("target")).unwrap();
    fs::write(directory.path().join("README.md"), "A small fixture\n").unwrap();
    fs::write(directory.path().join("src/lib.rs"), "pub fn answer() -> u8 { 42 }\n").unwrap();
    fs::write(directory.path().join("target/generated.txt"), "ignored build output\n").unwrap();

    let workspace = WorkspaceTools::new(directory.path()).unwrap();
    let mut registry = ToolRegistry::new();
    workspace.register(&mut registry).unwrap();

    let listing = registry.get("list_files").unwrap().call(json!({})).await.unwrap();
    assert_eq!(listing["files"], json!(["README.md", "src/lib.rs"]));

    let file = registry
        .get("read_file")
        .unwrap()
        .call(json!({ "path": "src/lib.rs" }))
        .await
        .unwrap();
    assert_eq!(file["content"], "pub fn answer() -> u8 { 42 }\n");
    assert_eq!(file["truncated"], false);

    let search = registry
        .get("search_text")
        .unwrap()
        .call(json!({ "query": "answer" }))
        .await
        .unwrap();
    assert_eq!(search["results"][0]["path"], "src/lib.rs");
    assert_eq!(search["results"][0]["line"], 1);
}

#[tokio::test]
async fn workspace_tools_reject_paths_outside_the_root() {
    let parent = tempdir().unwrap();
    let workspace_path = parent.path().join("workspace");
    fs::create_dir(&workspace_path).unwrap();
    fs::write(parent.path().join("secret.txt"), "secret").unwrap();

    let workspace = WorkspaceTools::new(&workspace_path).unwrap();
    let mut registry = ToolRegistry::new();
    workspace.register(&mut registry).unwrap();

    let error = registry
        .get("read_file")
        .unwrap()
        .call(json!({ "path": "../secret.txt" }))
        .await
        .unwrap_err();
    assert!(error.message.contains("escapes the workspace root"));
}

#[tokio::test]
async fn read_file_truncates_at_a_utf8_boundary() {
    let directory = tempdir().unwrap();
    fs::write(directory.path().join("unicode.txt"), "aéz").unwrap();

    let workspace = WorkspaceTools::new(directory.path()).unwrap().with_max_output_bytes(2);
    let mut registry = ToolRegistry::new();
    workspace.register(&mut registry).unwrap();

    let file = registry
        .get("read_file")
        .unwrap()
        .call(json!({ "path": "unicode.txt" }))
        .await
        .unwrap();
    assert_eq!(file["content"], "a");
    assert_eq!(file["truncated"], true);
}

#[tokio::test]
async fn workspace_walk_respects_gitignore_outside_a_git_repository() {
    let directory = tempdir().unwrap();
    fs::write(directory.path().join(".gitignore"), "ignored.txt\n").unwrap();
    fs::write(directory.path().join("included.txt"), "included\n").unwrap();
    fs::write(directory.path().join("ignored.txt"), "secret\n").unwrap();

    let workspace = WorkspaceTools::new(directory.path()).unwrap();
    let mut registry = ToolRegistry::new();
    workspace.register(&mut registry).unwrap();

    let listing = registry.get("list_files").unwrap().call(json!({})).await.unwrap();
    assert_eq!(listing["files"], json!([".gitignore", "included.txt"]));

    let search = registry
        .get("search_text")
        .unwrap()
        .call(json!({ "query": "secret" }))
        .await
        .unwrap();
    assert!(search["results"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn parallel_search_preserves_path_order_and_result_limits() {
    let directory = tempdir().unwrap();
    fs::write(directory.path().join("b.txt"), "match b1\nmatch b2\n").unwrap();
    fs::write(directory.path().join("a.txt"), "match a1\nmatch a2\n").unwrap();

    let workspace = WorkspaceTools::new(directory.path()).unwrap();
    let mut registry = ToolRegistry::new();
    workspace.register(&mut registry).unwrap();

    let search = registry
        .get("search_text")
        .unwrap()
        .call(json!({ "query": "match", "max_results": 3 }))
        .await
        .unwrap();

    assert_eq!(
        search["results"],
        json!([
            { "path": "a.txt", "line": 1, "text": "match a1" },
            { "path": "a.txt", "line": 2, "text": "match a2" },
            { "path": "b.txt", "line": 1, "text": "match b1" },
        ])
    );
    assert_eq!(search["truncated"], true);
}
