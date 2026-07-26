# Autocommit

Codex Autocommit batches working-tree changes from overlapping or sequential
Codex sessions into Git commits as soon as no active session has uncommitted
writes.

## Behavior

- Session state lives in `<git-dir>/codex-autocommit/sessions/<session-id>.json`.
- `SessionStart` installs the hook executable at a stable path in
  `$PLUGIN_DATA`, so reinstalling the plugin cannot strand an older session.
- Tool hooks track whether each active session has changed the committable
  working tree.
- `SessionEnd` writes a small, lock-free end marker without invoking Git, then
  starts a detached worker. The worker folds the marker into session state and
  performs any Git work.
- Each session records the owning Codex process ID. A session whose owner is no
  longer alive is deleted and cannot continue holding the commit lock.
- An active session blocks a commit only after it writes, or while one of its
  potentially writing tools is still running.
- A read-only active session does not prevent an ended session's changes from
  being committed.
- Existing staged, unstaged, and untracked changes intentionally join the batch.
- After a writing session ends, a detached worker resumes that exact Codex
  session with hooks disabled, read-only sandboxing, and ephemeral output to
  generate a schema-constrained commit subject and optional short body.
- Summary generation runs in the background and does not delay session
  shutdown. The commit waits for the result and falls back to a concise
  changed-path subject if Codex fails or times out.
- Session summaries exclude changed-path lists, diff statistics, and session
  counts. Chat transcripts and prompt text are not copied into plugin state.
- Unmerged paths or failed Git hooks leave the batch pending and write details
  to `<git-dir>/codex-autocommit/last_error.json`.

## Configuration

The hook process recognizes this environment variable:

- `CODEX_AUTOCOMMIT_STALE_SECONDS` (default `86400`) is the fallback for
  records without a detectable dead owner process.
- `CODEX_AUTOCOMMIT_SUMMARY_SECONDS` (default `120`) limits the background
  Codex turn used to summarize an ended writing session.
- `CODEX_AUTOCOMMIT_SUMMARY_DELAY_SECONDS` (default `1`) lets session shutdown
  finish before the detached worker resumes it.

Git's configured author and normal repository hooks are used for every commit.

## Development

```sh
cargo test --manifest-path plugins/autocommit/Cargo.toml
./install
```

Run these commands from the repository root. The root installer rebuilds and
overwrites the bundled `bin/codex-autocommit` executable before reinstalling the
plugin.
