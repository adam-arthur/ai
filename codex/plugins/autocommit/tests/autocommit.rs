use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

struct TestRepo {
    _temporary: tempfile::TempDir,
    path: PathBuf,
    initial_head: String,
}

impl TestRepo {
    fn new() -> Self {
        let temporary = tempfile::tempdir().unwrap();
        let path = temporary.path().to_path_buf();
        run(&path, "git", &["init", "-q"]);
        run(&path, "git", &["config", "user.name", "Codex Test"]);
        run(
            &path,
            "git",
            &["config", "user.email", "codex@example.invalid"],
        );
        fs::write(path.join("base.txt"), "base\n").unwrap();
        run(&path, "git", &["add", "base.txt"]);
        run(&path, "git", &["commit", "-qm", "initial"]);
        let initial_head = stdout(&path, "git", &["rev-parse", "HEAD"]);
        Self {
            _temporary: temporary,
            path,
            initial_head,
        }
    }

    fn event(&self, name: &str, session: &str) {
        self.event_with_owner(name, session, std::process::id());
    }

    fn event_with_owner(&self, name: &str, session: &str, owner_pid: u32) {
        let input = serde_json::json!({
            "hook_event_name": name,
            "session_id": session,
            "cwd": self.path,
        })
        .to_string();
        let mut child = command(&self.path, binary(), &["event"])
            .env("CODEX_AUTOCOMMIT_OWNER_PID", owner_pid.to_string())
            .stdin(Stdio::piped())
            .spawn()
            .unwrap();
        use std::io::Write;
        child
            .stdin
            .take()
            .unwrap()
            .write_all(input.as_bytes())
            .unwrap();
        assert!(child.wait().unwrap().success());
    }

    fn state_root(&self) -> PathBuf {
        PathBuf::from(stdout(
            &self.path,
            "git",
            &["rev-parse", "--absolute-git-dir"],
        ))
        .join("codex-autocommit")
    }

    fn worker(&self) -> Output {
        command(
            &self.path,
            binary(),
            &["worker", self.path.to_str().unwrap()],
        )
        .output()
        .unwrap()
    }
}

fn binary() -> &'static str {
    env!("CARGO_BIN_EXE_codex-autocommit")
}

fn command(cwd: &Path, program: &str, args: &[&str]) -> Command {
    let mut command = Command::new(program);
    command
        .args(args)
        .current_dir(cwd)
        .env("CODEX_AUTOCOMMIT_NO_SPAWN", "1")
        .env("CODEX_AUTOCOMMIT_NO_SUMMARY", "1")
        .env("CODEX_AUTOCOMMIT_SUMMARY_DELAY_SECONDS", "0");
    command
}

fn run(cwd: &Path, program: &str, args: &[&str]) -> Output {
    let output = command(cwd, program, args).output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

fn stdout(cwd: &Path, program: &str, args: &[&str]) -> String {
    String::from_utf8(run(cwd, program, args).stdout)
        .unwrap()
        .trim()
        .to_owned()
}

#[cfg(unix)]
fn fake_codex(script: &str) -> (tempfile::TempDir, PathBuf) {
    use std::os::unix::fs::PermissionsExt;

    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("codex");
    fs::write(&path, script).unwrap();
    let mut permissions = fs::metadata(&path).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&path, permissions).unwrap();
    (directory, path)
}

#[test]
fn read_only_active_session_does_not_block_commit() {
    let repo = TestRepo::new();
    fs::write(repo.path.join("manual.txt"), "manual\n").unwrap();
    repo.event("SessionStart", "one");
    repo.event("SessionStart", "two");
    fs::write(repo.path.join("one.txt"), "one\n").unwrap();
    repo.event("SessionEnd", "one");

    let state_root = repo.state_root();
    assert!(state_root.join("ended/one.json").is_file());
    let record: serde_json::Value =
        serde_json::from_slice(&fs::read(state_root.join("sessions/one.json")).unwrap()).unwrap();
    assert_eq!(record["active"], true);

    assert!(repo.worker().status.success());
    assert!(!state_root.join("ended/one.json").exists());
    let committed: std::collections::BTreeSet<_> = stdout(
        &repo.path,
        "git",
        &["show", "--pretty=", "--name-only", "HEAD"],
    )
    .lines()
    .map(str::to_owned)
    .collect();
    assert_eq!(
        committed,
        ["manual.txt", "one.txt"].map(str::to_owned).into()
    );

    repo.event("SessionEnd", "two");
    assert!(repo.worker().status.success());
    assert_eq!(stdout(&repo.path, "git", &["status", "--porcelain"]), "");
}

#[test]
fn dead_owner_does_not_block_commit() {
    let repo = TestRepo::new();
    repo.event("SessionStart", "ended");
    repo.event_with_owner("SessionStart", "orphan", u32::MAX);
    repo.event_with_owner("PreToolUse", "orphan", u32::MAX);
    fs::write(repo.path.join("orphan.txt"), "orphan\n").unwrap();
    repo.event_with_owner("PostToolUse", "orphan", u32::MAX);
    repo.event("SessionEnd", "ended");

    assert!(repo.worker().status.success());
    assert!(!repo.state_root().join("sessions/orphan.json").exists());
    assert_ne!(
        stdout(&repo.path, "git", &["rev-parse", "HEAD"]),
        repo.initial_head
    );
    assert_eq!(stdout(&repo.path, "git", &["status", "--porcelain"]), "");
}

#[test]
fn orphan_record_is_deleted() {
    let repo = TestRepo::new();
    repo.event_with_owner("SessionStart", "orphan", u32::MAX);

    assert!(repo.worker().status.success());
    assert!(!repo.state_root().join("sessions/orphan.json").exists());
    assert_eq!(
        stdout(&repo.path, "git", &["rev-parse", "HEAD"]),
        repo.initial_head
    );
}

#[test]
fn session_end_does_not_require_git() {
    let repo = TestRepo::new();
    repo.event("SessionStart", "ending");
    let input = serde_json::json!({
        "hook_event_name": "SessionEnd",
        "session_id": "ending",
        "cwd": repo.path,
    })
    .to_string();
    let mut child = command(&repo.path, binary(), &["event"])
        .env("PATH", "/path/that/does/not/exist")
        .stdin(Stdio::piped())
        .spawn()
        .unwrap();
    use std::io::Write;
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();

    assert!(child.wait().unwrap().success());
    assert!(repo.state_root().join("ended/ending.json").is_file());
}

#[test]
fn active_session_that_wrote_blocks_commit() {
    let repo = TestRepo::new();
    repo.event("SessionStart", "one");
    repo.event("SessionStart", "two");
    fs::write(repo.path.join("one.txt"), "one\n").unwrap();
    repo.event("PreToolUse", "two");
    fs::write(repo.path.join("two.txt"), "two\n").unwrap();
    repo.event("PostToolUse", "two");
    // Compaction and resume can emit SessionStart again for the same session.
    repo.event("SessionStart", "two");
    repo.event("SessionEnd", "one");

    assert!(repo.worker().status.success());
    assert_eq!(
        stdout(&repo.path, "git", &["rev-parse", "HEAD"]),
        repo.initial_head
    );

    repo.event("SessionEnd", "two");
    assert!(repo.worker().status.success());
    let committed: std::collections::BTreeSet<_> = stdout(
        &repo.path,
        "git",
        &["show", "--pretty=", "--name-only", "HEAD"],
    )
    .lines()
    .map(str::to_owned)
    .collect();
    assert_eq!(committed, ["one.txt", "two.txt"].map(str::to_owned).into());
    assert_eq!(stdout(&repo.path, "git", &["status", "--porcelain"]), "");
}

#[test]
fn no_changes_produces_no_commit() {
    let repo = TestRepo::new();
    repo.event("SessionStart", "empty");
    repo.event("SessionEnd", "empty");
    assert!(repo.worker().status.success());
    assert_eq!(
        stdout(&repo.path, "git", &["rev-parse", "HEAD"]),
        repo.initial_head
    );
}

#[test]
fn subject_is_concise() {
    let repo = TestRepo::new();
    repo.event("SessionStart", "subject");
    fs::create_dir(repo.path.join("src")).unwrap();
    fs::write(repo.path.join("src/feature.rs"), "fn main() {}\n").unwrap();
    repo.event("SessionEnd", "subject");
    assert!(repo.worker().status.success());
    let subject = stdout(&repo.path, "git", &["log", "-1", "--pretty=%s"]);
    assert_eq!(subject, "codex: update src");
    assert!(subject.len() <= 72);
    assert_eq!(
        stdout(&repo.path, "git", &["log", "-1", "--pretty=%B"]),
        "codex: update src"
    );
}

#[cfg(unix)]
#[test]
fn worker_uses_ended_session_summary() {
    let repo = TestRepo::new();
    repo.event("SessionStart", "summarized");
    repo.event("PreToolUse", "summarized");
    fs::write(repo.path.join("feature.txt"), "feature\n").unwrap();
    repo.event("PostToolUse", "summarized");
    repo.event("SessionEnd", "summarized");

    let (_fake_directory, fake_codex) = fake_codex(
        r#"#!/bin/sh
output=
while [ "$#" -gt 0 ]; do
  if [ "$1" = "--output-last-message" ]; then
    shift
    output=$1
  fi
  shift
done
printf '%s\n' '{"subject":"fix: summarize completed sessions","body":""}' > "$output"
"#,
    );

    let output = command(
        &repo.path,
        binary(),
        &["worker", repo.path.to_str().unwrap()],
    )
    .env_remove("CODEX_AUTOCOMMIT_NO_SUMMARY")
    .env("CODEX_AUTOCOMMIT_CODEX", &fake_codex)
    .output()
    .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        stdout(&repo.path, "git", &["log", "-1", "--pretty=%B"]),
        "fix: summarize completed sessions"
    );
    let last_commit: serde_json::Value =
        serde_json::from_slice(&fs::read(repo.state_root().join("last_commit.json")).unwrap())
            .unwrap();
    assert_eq!(last_commit["summarized"], true);
}

#[cfg(unix)]
#[test]
fn session_end_does_not_wait_for_summary_generation() {
    let repo = TestRepo::new();
    repo.event("SessionStart", "background");
    repo.event("PreToolUse", "background");
    fs::write(repo.path.join("background.txt"), "background\n").unwrap();
    repo.event("PostToolUse", "background");

    let (_fake_directory, fake_codex) = fake_codex(
        r#"#!/bin/sh
output=
while [ "$#" -gt 0 ]; do
  if [ "$1" = "--output-last-message" ]; then
    shift
    output=$1
  fi
  shift
done
sleep 1
printf '%s\n' '{"subject":"fix: finish summaries in the background","body":""}' > "$output"
"#,
    );
    let input = serde_json::json!({
        "hook_event_name": "SessionEnd",
        "session_id": "background",
        "cwd": repo.path,
    })
    .to_string();
    let mut child = command(&repo.path, binary(), &["event"])
        .env_remove("CODEX_AUTOCOMMIT_NO_SPAWN")
        .env_remove("CODEX_AUTOCOMMIT_NO_SUMMARY")
        .env("CODEX_AUTOCOMMIT_CODEX", &fake_codex)
        .stdin(Stdio::piped())
        .spawn()
        .unwrap();
    use std::io::Write;
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
    assert!(child.wait().unwrap().success());
    assert_eq!(
        stdout(&repo.path, "git", &["rev-parse", "HEAD"]),
        repo.initial_head
    );

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while stdout(&repo.path, "git", &["rev-parse", "HEAD"]) == repo.initial_head
        && std::time::Instant::now() < deadline
    {
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    assert_eq!(
        stdout(&repo.path, "git", &["log", "-1", "--pretty=%B"]),
        "fix: finish summaries in the background"
    );
}
