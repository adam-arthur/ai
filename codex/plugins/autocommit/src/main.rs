use fs2::FileExt;
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use std::env;
use std::ffi::OsStr;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[cfg(unix)]
use std::os::unix::process::CommandExt;

const DEFAULT_STALE_SECONDS: u64 = 86_400;
const DEFAULT_SUMMARY_DELAY_SECONDS: u64 = 1;
const DEFAULT_SUMMARY_SECONDS: u64 = 120;
const STATE_DIR_NAME: &str = "codex-autocommit";
const SUMMARY_PROMPT: &str = "Summarize the repository work completed in this Codex session for a Git commit. Return JSON with a concise Conventional Commit subject (72 characters maximum) and an optional short body. Describe purpose and outcome, not changed paths, file counts, line counts, session counts, or implementation bookkeeping. Use an empty body unless it adds important motivation or a non-obvious constraint. Do not use tools or modify files.";

#[derive(Clone)]
struct SummaryJob {
    path: PathBuf,
    session_id: String,
    model: Option<String>,
    ended_at: f64,
}

struct CommitSummary {
    subject: String,
    body: String,
}

fn git<I, S>(cwd: &Path, args: I) -> io::Result<Output>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    Command::new("git").args(args).current_dir(cwd).output()
}

fn repository(cwd: &Path) -> Option<(PathBuf, PathBuf)> {
    let root_output = git(cwd, ["rev-parse", "--show-toplevel"]).ok()?;
    if !root_output.status.success() {
        return None;
    }
    let root = fs::canonicalize(String::from_utf8_lossy(&root_output.stdout).trim()).ok()?;
    let git_dir_output = git(&root, ["rev-parse", "--absolute-git-dir"]).ok()?;
    if !git_dir_output.status.success() {
        return None;
    }
    let git_dir = fs::canonicalize(String::from_utf8_lossy(&git_dir_output.stdout).trim()).ok()?;
    Some((root, git_dir))
}

fn repository_from_metadata(cwd: &Path) -> Option<(PathBuf, PathBuf)> {
    let cwd = fs::canonicalize(cwd).ok()?;
    let start = if cwd.is_dir() {
        cwd.as_path()
    } else {
        cwd.parent()?
    };
    for candidate in start.ancestors() {
        let dot_git = candidate.join(".git");
        let git_dir = if dot_git.is_dir() {
            fs::canonicalize(dot_git).ok()?
        } else if dot_git.is_file() {
            let contents = fs::read_to_string(&dot_git).ok()?;
            let path = contents.trim().strip_prefix("gitdir:")?.trim();
            let path = Path::new(path);
            let path = if path.is_absolute() {
                path.to_path_buf()
            } else {
                candidate.join(path)
            };
            fs::canonicalize(path).ok()?
        } else {
            continue;
        };
        return Some((fs::canonicalize(candidate).ok()?, git_dir));
    }
    None
}

fn now_seconds() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
}

fn state_root(git_dir: &Path) -> PathBuf {
    git_dir.join(STATE_DIR_NAME)
}

fn session_filename(session_id: &str) -> String {
    let safe = !session_id.is_empty()
        && session_id.len() <= 128
        && session_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte));
    if safe {
        format!("{session_id}.json")
    } else {
        format!("{:x}.json", Sha256::digest(session_id.as_bytes()))
    }
}

fn read_json(path: &Path) -> Value {
    fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_else(|| json!({}))
}

fn write_json(path: &Path, value: &Value) -> io::Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let mut attempt = 0_u32;
    let (temporary, mut file) = loop {
        let name = format!(
            ".{}.{}.{}.tmp",
            path.file_name().and_then(OsStr::to_str).unwrap_or("state"),
            std::process::id(),
            attempt
        );
        let temporary = parent.join(name);
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
        {
            Ok(file) => break (temporary, file),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => attempt += 1,
            Err(error) => return Err(error),
        }
    };
    let result = (|| {
        serde_json::to_writer_pretty(&mut file, value)?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temporary, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

struct StateLock(File);

impl StateLock {
    fn acquire(root: &Path) -> io::Result<Self> {
        fs::create_dir_all(root)?;
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(root.join("lock"))?;
        file.lock_exclusive()?;
        Ok(Self(file))
    }
}

impl Drop for StateLock {
    fn drop(&mut self) {
        let _ = self.0.unlock();
    }
}

fn env_seconds(name: &str, default: u64) -> u64 {
    env::var(name)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}

fn nul_paths(output: &[u8]) -> Vec<String> {
    output
        .split(|byte| *byte == 0)
        .filter(|part| !part.is_empty())
        .map(|part| String::from_utf8_lossy(part).into_owned())
        .collect()
}

fn worktree_fingerprint(repo: &Path) -> String {
    let mut digest = Sha256::new();
    if let Ok(output) = git(repo, ["diff", "--binary", "HEAD", "--"]) {
        digest.update(&output.stdout);
    }
    if let Ok(output) = git(repo, ["ls-files", "--others", "--exclude-standard", "-z"])
        && output.status.success()
    {
        for path in nul_paths(&output.stdout) {
            digest.update(path.as_bytes());
            digest.update([0]);
            let full_path = repo.join(&path);
            if let Ok(metadata) = fs::symlink_metadata(&full_path) {
                if metadata.file_type().is_symlink() {
                    if let Ok(target) = fs::read_link(full_path) {
                        digest.update(target.as_os_str().as_encoded_bytes());
                    }
                } else if let Ok(contents) = fs::read(full_path) {
                    digest.update(contents);
                }
            }
            digest.update([0]);
        }
    }
    format!("{:x}", digest.finalize())
}

fn object(value: Value) -> Map<String, Value> {
    value.as_object().cloned().unwrap_or_default()
}

fn owner_pid() -> Option<u32> {
    env::var("CODEX_AUTOCOMMIT_OWNER_PID")
        .ok()
        .and_then(|value| value.parse().ok())
        .filter(|pid| *pid > 1)
}

#[cfg(unix)]
fn process_is_alive(pid: u32) -> bool {
    let Ok(pid) = i32::try_from(pid) else {
        return false;
    };
    let result = unsafe { libc::kill(pid, 0) };
    result == 0 || io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

#[cfg(not(unix))]
fn process_is_alive(_pid: u32) -> bool {
    true
}

fn end_marker_path(root: &Path, session_id: &str) -> PathBuf {
    root.join("ended").join(session_filename(session_id))
}

fn apply_end_markers(root: &Path) -> io::Result<()> {
    let ended = root.join("ended");
    let Ok(entries) = fs::read_dir(&ended) else {
        return Ok(());
    };
    for entry in entries.flatten() {
        let marker_path = entry.path();
        if marker_path.extension() != Some(OsStr::new("json")) {
            continue;
        }
        let marker = read_json(&marker_path);
        let Some(session_id) = marker.get("session_id").and_then(Value::as_str) else {
            continue;
        };
        let ended_at = marker
            .get("ended_at")
            .and_then(Value::as_f64)
            .unwrap_or_else(now_seconds);
        let session_path = root.join("sessions").join(session_filename(session_id));
        let mut record = object(read_json(&session_path));
        record.insert("session_id".into(), json!(session_id));
        record.insert("active".into(), json!(false));
        record.insert("writing".into(), json!(false));
        record.insert("last_seen".into(), json!(ended_at));
        record.insert("ended_at".into(), json!(ended_at));
        if let Some(model) = marker.get("model").and_then(Value::as_str) {
            record.insert("model".into(), json!(model));
        }
        record.remove("pre_tool_fingerprint");
        write_json(&session_path, &Value::Object(record))?;
        fs::remove_file(marker_path)?;
    }
    Ok(())
}

fn mark_stale_sessions(root: &Path, now: f64) -> io::Result<()> {
    let stale_after = env_seconds("CODEX_AUTOCOMMIT_STALE_SECONDS", DEFAULT_STALE_SECONDS) as f64;
    let sessions = root.join("sessions");
    let Ok(entries) = fs::read_dir(sessions) else {
        return Ok(());
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension() != Some(OsStr::new("json")) {
            continue;
        }
        let record = object(read_json(&path));
        let active = record
            .get("active")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let last_seen = record
            .get("last_seen")
            .and_then(Value::as_f64)
            .unwrap_or(now);
        let owner_dead = record
            .get("owner_pid")
            .and_then(Value::as_u64)
            .and_then(|pid| u32::try_from(pid).ok())
            .is_some_and(|pid| !process_is_alive(pid));
        if active && (owner_dead || now - last_seen >= stale_after) {
            fs::remove_file(path)?;
        }
    }
    Ok(())
}

fn payload_string<'a>(payload: &'a Value, key: &str) -> Option<&'a str> {
    payload
        .get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
}

fn handle_event(payload: &Value) -> io::Result<i32> {
    let cwd = payload_string(payload, "cwd")
        .map(PathBuf::from)
        .unwrap_or(env::current_dir()?);
    let session_id = payload_string(payload, "session_id").unwrap_or("unknown");
    let event = payload_string(payload, "hook_event_name").unwrap_or("");
    let model = payload_string(payload, "model");
    let now = now_seconds();

    if event == "SessionEnd" {
        let Some((repo, git_dir)) = repository_from_metadata(&cwd) else {
            return Ok(0);
        };
        let root = state_root(&git_dir);
        write_json(
            &end_marker_path(&root, session_id),
            &json!({"session_id": session_id, "ended_at": now, "model": model}),
        )?;
        spawn_worker(&repo)?;
        return Ok(0);
    }

    let Some((repo, git_dir)) = repository(&cwd) else {
        return Ok(0);
    };
    let root = state_root(&git_dir);
    let session_path = root.join("sessions").join(session_filename(session_id));
    let should_spawn;

    {
        let _lock = StateLock::acquire(&root)?;
        apply_end_markers(&root)?;
        mark_stale_sessions(&root, now)?;
        let mut record = object(read_json(&session_path));
        record.insert("session_id".into(), json!(session_id));
        record.insert("repo".into(), json!(repo));
        record.insert("last_seen".into(), json!(now));
        record.entry("started_at").or_insert_with(|| json!(now));
        if let Some(pid) = owner_pid() {
            record.insert("owner_pid".into(), json!(pid));
        }
        if let Some(model) = model {
            record.insert("model".into(), json!(model));
        }

        match event {
            "SessionStart" => {
                let _ = fs::remove_file(end_marker_path(&root, session_id));
                record.insert("active".into(), json!(true));
                record.entry("wrote").or_insert_with(|| json!(false));
                record.insert("writing".into(), json!(false));
                record.remove("pre_tool_fingerprint");
                record.remove("ended_at");
                record.remove("stale");
                write_json(&session_path, &Value::Object(record))?;
                return Ok(0);
            }
            "PreToolUse" => {
                record.insert("writing".into(), json!(true));
                write_json(&session_path, &Value::Object(record.clone()))?;
                record.insert(
                    "pre_tool_fingerprint".into(),
                    json!(worktree_fingerprint(&repo)),
                );
                write_json(&session_path, &Value::Object(record))?;
                return Ok(0);
            }
            "PostToolUse" => {
                let before = record.get("pre_tool_fingerprint").and_then(Value::as_str);
                let changed = before.is_none_or(|before| before != worktree_fingerprint(&repo));
                let wrote = record
                    .get("wrote")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                record.insert("wrote".into(), json!(wrote || changed));
                record.insert("writing".into(), json!(false));
                record.remove("pre_tool_fingerprint");
                write_json(&session_path, &Value::Object(record))?;
            }
            _ => return Ok(0),
        }
        should_spawn = session_records(&root).iter().any(|(_, record)| {
            !record
                .get("active")
                .and_then(Value::as_bool)
                .unwrap_or(false)
        });
    }

    if should_spawn {
        spawn_worker(&repo)?;
    }
    Ok(0)
}

fn spawn_worker(repo: &Path) -> io::Result<()> {
    if env::var("CODEX_AUTOCOMMIT_NO_SPAWN").as_deref() == Ok("1") {
        return Ok(());
    }
    let executable = env::current_exe()?;
    let mut command = Command::new(executable);
    command
        .arg("worker")
        .arg(repo)
        .current_dir(repo)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(unix)]
    command.process_group(0);
    command.spawn()?;
    Ok(())
}

fn git_stdout(repo: &Path, args: &[&str]) -> Vec<u8> {
    git(repo, args)
        .map(|output| output.stdout)
        .unwrap_or_default()
}

fn has_changes(repo: &Path) -> bool {
    !git_stdout(repo, &["status", "--porcelain=v1", "-z"]).is_empty()
}

fn has_conflicts(repo: &Path) -> bool {
    !git_stdout(repo, &["ls-files", "-u"]).is_empty()
}

fn session_records(root: &Path) -> Vec<(PathBuf, Value)> {
    let Ok(entries) = fs::read_dir(root.join("sessions")) else {
        return Vec::new();
    };
    let mut paths: Vec<_> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension() == Some(OsStr::new("json")))
        .collect();
    paths.sort();
    paths
        .into_iter()
        .map(|path| {
            let value = read_json(&path);
            (path, value)
        })
        .collect()
}

fn concise_subject(paths: &[String]) -> String {
    if paths.is_empty() {
        return "codex: update working tree".into();
    }
    let mut areas = Vec::new();
    for path in paths {
        let candidate = Path::new(path);
        let area = if candidate.components().count() > 1 {
            candidate.components().next().map(|part| part.as_os_str())
        } else {
            candidate.file_name()
        };
        let Some(area) = area.and_then(OsStr::to_str) else {
            continue;
        };
        if !area.is_empty() && !areas.contains(&area) {
            areas.push(area);
        }
    }
    let mut label = areas.iter().take(3).copied().collect::<Vec<_>>().join(", ");
    if areas.len() > 3 {
        label.push_str(&format!(" and {} more", areas.len() - 3));
    }
    let subject = format!("codex: update {label}");
    subject
        .chars()
        .take(72)
        .collect::<String>()
        .trim_end_matches([' ', ','])
        .into()
}

fn summary_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "subject": {"type": "string"},
            "body": {"type": "string"}
        },
        "required": ["subject", "body"],
        "additionalProperties": false
    })
}

fn parse_summary(value: &Value) -> Result<CommitSummary, &'static str> {
    let subject = value
        .get("subject")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or("");
    let body = value
        .get("body")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or("");
    if subject.is_empty() {
        return Err("empty subject");
    }
    if subject.lines().count() != 1 {
        return Err("subject contains a newline");
    }
    if subject.chars().count() > 72 {
        return Err("subject exceeds 72 characters");
    }
    if body.chars().count() > 1_000 {
        return Err("body exceeds 1000 characters");
    }
    Ok(CommitSummary {
        subject: subject.to_owned(),
        body: body.to_owned(),
    })
}

fn summaries_enabled() -> bool {
    env::var("CODEX_AUTOCOMMIT_NO_SUMMARY").as_deref() != Ok("1")
}

#[cfg(unix)]
fn terminate_child(child: &mut std::process::Child) {
    if let Ok(pid) = i32::try_from(child.id()) {
        unsafe {
            libc::kill(-pid, libc::SIGKILL);
        }
    }
    let _ = child.wait();
}

#[cfg(not(unix))]
fn terminate_child(child: &mut std::process::Child) {
    let _ = child.kill();
    let _ = child.wait();
}

fn generate_summary(repo: &Path, root: &Path, job: &SummaryJob) -> io::Result<CommitSummary> {
    let start_after = job.ended_at
        + env_seconds(
            "CODEX_AUTOCOMMIT_SUMMARY_DELAY_SECONDS",
            DEFAULT_SUMMARY_DELAY_SECONDS,
        ) as f64;
    let delay = start_after - now_seconds();
    if delay > 0.0 {
        thread::sleep(Duration::from_secs_f64(delay));
    }
    let schema_path = root.join("summary-schema.json");
    write_json(&schema_path, &summary_schema())?;
    let output_path = root
        .join("summaries")
        .join(session_filename(&job.session_id));
    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent)?;
    }
    let _ = fs::remove_file(&output_path);

    let codex = env::var_os("CODEX_AUTOCOMMIT_CODEX").unwrap_or_else(|| "codex".into());
    let mut command = Command::new(codex);
    command
        .arg("exec")
        .arg("--disable")
        .arg("hooks")
        .arg("--sandbox")
        .arg("read-only")
        .arg("--ephemeral")
        .arg("--color")
        .arg("never");
    if let Some(model) = &job.model {
        command.arg("--model").arg(model);
    }
    command
        .arg("resume")
        .arg("--output-schema")
        .arg(&schema_path)
        .arg("--output-last-message")
        .arg(&output_path)
        .arg(&job.session_id)
        .arg(SUMMARY_PROMPT)
        .current_dir(repo)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(unix)]
    command.process_group(0);
    let mut child = command.spawn()?;
    let timeout = Duration::from_secs(env_seconds(
        "CODEX_AUTOCOMMIT_SUMMARY_SECONDS",
        DEFAULT_SUMMARY_SECONDS,
    ));
    let started = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if started.elapsed() >= timeout {
            terminate_child(&mut child);
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "Codex session summary timed out",
            ));
        }
        thread::sleep(Duration::from_millis(100));
    };
    if !status.success() {
        return Err(io::Error::other(format!(
            "Codex session summary exited with {status}"
        )));
    }
    let value = read_json(&output_path);
    let _ = fs::remove_file(output_path);
    parse_summary(&value).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

fn claim_summary_jobs(records: &[(PathBuf, Value)]) -> io::Result<Vec<SummaryJob>> {
    let mut jobs = Vec::new();
    for (path, value) in records {
        let mut record = object(value.clone());
        if record
            .get("active")
            .and_then(Value::as_bool)
            .unwrap_or(false)
            || !record.get("wrote").and_then(Value::as_bool).unwrap_or(true)
        {
            continue;
        }
        let status = record
            .get("summary")
            .and_then(|value| value.get("status"))
            .and_then(Value::as_str);
        if status.is_some() {
            continue;
        }
        let Some(session_id) = record
            .get("session_id")
            .and_then(Value::as_str)
            .map(str::to_owned)
        else {
            continue;
        };
        if !summaries_enabled() {
            record.insert(
                "summary".into(),
                json!({"status": "failed", "error": "summary disabled"}),
            );
            write_json(path, &Value::Object(record))?;
            continue;
        }
        let model = record
            .get("model")
            .and_then(Value::as_str)
            .map(str::to_owned);
        let ended_at = record
            .get("ended_at")
            .and_then(Value::as_f64)
            .unwrap_or_else(now_seconds);
        record.insert(
            "summary".into(),
            json!({
                "status": "running",
                "started_at": now_seconds(),
                "worker_pid": std::process::id()
            }),
        );
        write_json(path, &Value::Object(record))?;
        jobs.push(SummaryJob {
            path: path.clone(),
            session_id,
            model,
            ended_at,
        });
    }
    Ok(jobs)
}

fn finish_summary(job: &SummaryJob, result: io::Result<CommitSummary>) -> io::Result<()> {
    let mut record = object(read_json(&job.path));
    let summary = match result {
        Ok(summary) => json!({
            "status": "ready",
            "subject": summary.subject,
            "body": summary.body,
            "completed_at": now_seconds()
        }),
        Err(error) => json!({
            "status": "failed",
            "error": error.to_string(),
            "completed_at": now_seconds()
        }),
    };
    record.insert("summary".into(), summary);
    write_json(&job.path, &Value::Object(record))
}

fn summary_is_running(record: &Value) -> bool {
    record
        .get("summary")
        .and_then(|value| value.get("status"))
        .and_then(Value::as_str)
        == Some("running")
}

fn recover_abandoned_summaries(records: &[(PathBuf, Value)], now: f64) -> io::Result<()> {
    let stale_after =
        env_seconds("CODEX_AUTOCOMMIT_SUMMARY_SECONDS", DEFAULT_SUMMARY_SECONDS) as f64 + 30.0;
    for (path, value) in records {
        if !summary_is_running(value) {
            continue;
        }
        let summary = value.get("summary").unwrap_or(&Value::Null);
        let worker_dead = summary
            .get("worker_pid")
            .and_then(Value::as_u64)
            .and_then(|pid| u32::try_from(pid).ok())
            .is_some_and(|pid| !process_is_alive(pid));
        let started_at = summary
            .get("started_at")
            .and_then(Value::as_f64)
            .unwrap_or(now);
        if !worker_dead && now - started_at < stale_after {
            continue;
        }
        let mut record = object(value.clone());
        record.insert(
            "summary".into(),
            json!({
                "status": "failed",
                "error": "summary worker ended before producing output",
                "completed_at": now
            }),
        );
        write_json(path, &Value::Object(record))?;
    }
    Ok(())
}

fn commit_summary(records: &[(PathBuf, Value)]) -> Option<CommitSummary> {
    records
        .iter()
        .filter_map(|(_, record)| {
            let summary = record.get("summary")?;
            if summary.get("status").and_then(Value::as_str) != Some("ready") {
                return None;
            }
            let parsed = parse_summary(summary).ok()?;
            let ended_at = record
                .get("ended_at")
                .and_then(Value::as_f64)
                .unwrap_or(0.0);
            Some((ended_at, parsed))
        })
        .max_by(|left, right| left.0.total_cmp(&right.0))
        .map(|(_, summary)| summary)
}

fn clean_completed_state(root: &Path, records: &[(PathBuf, Value)]) {
    for (path, record) in records {
        if !record
            .get("active")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            let _ = fs::remove_file(path);
        }
    }
    let _ = fs::remove_file(root.join("deadline.json"));
}

fn write_error(root: &Path, now: f64, error: impl AsRef<str>) -> io::Result<()> {
    write_json(
        &root.join("last_error.json"),
        &json!({"at": now, "error": error.as_ref()}),
    )
}

fn run_worker(repo: &Path) -> io::Result<i32> {
    let Some((repo, git_dir)) = repository(repo) else {
        return Ok(0);
    };
    let root = state_root(&git_dir);

    loop {
        let jobs = {
            let _lock = StateLock::acquire(&root)?;
            let now = now_seconds();
            apply_end_markers(&root)?;
            mark_stale_sessions(&root, now)?;
            let mut records = session_records(&root);
            recover_abandoned_summaries(&records, now)?;
            records = session_records(&root);
            if !records.iter().any(|(_, record)| {
                !record
                    .get("active")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
            }) {
                return Ok(0);
            }
            if !has_changes(&repo) {
                clean_completed_state(&root, &records);
                return Ok(0);
            }
            claim_summary_jobs(&records)?
        };

        if !jobs.is_empty() {
            for job in jobs {
                let result = generate_summary(&repo, &root, &job);
                let _lock = StateLock::acquire(&root)?;
                finish_summary(&job, result)?;
            }
            continue;
        }

        let _lock = StateLock::acquire(&root)?;
        let now = now_seconds();
        apply_end_markers(&root)?;
        mark_stale_sessions(&root, now)?;
        let mut records = session_records(&root);
        recover_abandoned_summaries(&records, now)?;
        records = session_records(&root);
        if records.iter().any(|(_, record)| summary_is_running(record)) {
            return Ok(0);
        }
        if records.iter().any(|(_, record)| {
            let active = record
                .get("active")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let wrote = record.get("wrote").and_then(Value::as_bool).unwrap_or(true);
            let writing = record
                .get("writing")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            active && (wrote || writing)
        }) {
            return Ok(0);
        }
        if !has_changes(&repo) {
            clean_completed_state(&root, &records);
            return Ok(0);
        }
        if has_conflicts(&repo) {
            write_error(&root, now, "unmerged paths")?;
            return Ok(1);
        }

        let added = git(&repo, ["add", "-A"])?;
        if !added.status.success() {
            write_error(&root, now, String::from_utf8_lossy(&added.stderr).trim())?;
            return Ok(added.status.code().unwrap_or(1));
        }
        let staged = git_stdout(&repo, &["diff", "--cached", "--name-only", "-z"]);
        let mut paths = nul_paths(&staged);
        paths.sort();
        if paths.is_empty() {
            clean_completed_state(&root, &records);
            return Ok(0);
        }

        let summary = commit_summary(&records);
        let subject = summary
            .as_ref()
            .map(|summary| summary.subject.clone())
            .unwrap_or_else(|| concise_subject(&paths));
        let mut message = subject.clone();
        if let Some(body) = summary
            .as_ref()
            .map(|summary| summary.body.trim())
            .filter(|body| !body.is_empty())
        {
            message.push_str("\n\n");
            message.push_str(body);
        }
        message.push('\n');
        let sessions = records
            .iter()
            .filter(|(_, record)| {
                !record
                    .get("active")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
            })
            .count();
        let message_path = root.join("commit-message.txt");
        fs::write(&message_path, message)?;
        let committed = git(
            &repo,
            [
                OsStr::new("commit"),
                OsStr::new("-F"),
                message_path.as_os_str(),
            ],
        )?;
        if !committed.status.success() {
            let stderr = String::from_utf8_lossy(&committed.stderr);
            let stdout = String::from_utf8_lossy(&committed.stdout);
            let detail = if stderr.trim().is_empty() {
                stdout.trim()
            } else {
                stderr.trim()
            };
            write_error(&root, now, detail)?;
            return Ok(committed.status.code().unwrap_or(1));
        }

        let commit = String::from_utf8_lossy(&git_stdout(&repo, &["rev-parse", "HEAD"]))
            .trim()
            .to_owned();
        write_json(
            &root.join("last_commit.json"),
            &json!({
                "at": now_seconds(),
                "commit": commit,
                "paths": paths,
                "sessions": sessions,
                "subject": subject,
                "summarized": summary.is_some()
            }),
        )?;
        let _ = fs::remove_file(root.join("last_error.json"));
        let _ = fs::remove_file(message_path);
        clean_completed_state(&root, &records);
        return Ok(0);
    }
}

fn usage() -> i32 {
    eprintln!("usage: codex-autocommit event | worker REPOSITORY");
    2
}

fn run() -> io::Result<i32> {
    let args: Vec<_> = env::args_os().collect();
    match args.get(1).and_then(|value| value.to_str()) {
        Some("event") => {
            let mut input = Vec::new();
            io::stdin().read_to_end(&mut input)?;
            let Ok(payload) = serde_json::from_slice(&input) else {
                return Ok(0);
            };
            handle_event(&payload)
        }
        Some("worker") if args.len() == 3 => run_worker(Path::new(&args[2])),
        _ => Ok(usage()),
    }
}

fn main() {
    let code = match run() {
        Ok(code) => code,
        Err(error) => {
            eprintln!("codex-autocommit: {error}");
            1
        }
    };
    std::process::exit(code);
}
