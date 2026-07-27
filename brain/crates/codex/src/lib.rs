//! A minimal [`brain`] runtime backed by `codex exec`.

#![forbid(unsafe_code)]

use std::{ffi::OsString, fs, path::Path};

use brain::{Access, AgentRuntime, Internet, RuntimeError, RuntimeRequest, RuntimeResponse, async_trait};
use serde_json::{Value, json};
use tempfile::tempdir;
use tokio::process::Command;

/// Invokes a locally installed Codex CLI in ephemeral, non-interactive mode.
#[derive(Clone, Debug)]
pub struct CodexRuntime {
    executable: OsString,
}

impl CodexRuntime {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn executable(mut self, executable: impl Into<OsString>) -> Self {
        self.executable = executable.into();
        self
    }
}

impl Default for CodexRuntime {
    fn default() -> Self {
        Self {
            executable: OsString::from("codex"),
        }
    }
}

#[async_trait]
impl AgentRuntime for CodexRuntime {
    async fn invoke(&self, request: RuntimeRequest) -> Result<RuntimeResponse, RuntimeError> {
        let temporary =
            tempdir().map_err(|error| RuntimeError::new(format!("failed to create temporary Codex files: {error}")))?;
        let schema_path = temporary.path().join("output.schema.json");
        let response_path = temporary.path().join("response.json");
        let schema = serde_json::to_string_pretty(&request.output_schema)
            .map_err(|error| RuntimeError::new(format!("failed to encode output schema: {error}")))?;
        fs::write(&schema_path, schema)
            .map_err(|error| RuntimeError::new(format!("failed to write output schema: {error}")))?;

        let mut command = Command::new(&self.executable);
        command
            .args(command_arguments(&request, &schema_path, &response_path))
            .current_dir(&request.working_directory)
            .kill_on_drop(true);

        let output = command.output().await.map_err(|error| {
            RuntimeError::new(format!(
                "failed to launch `{}`: {error}",
                self.executable.to_string_lossy()
            ))
        })?;
        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        let events = parse_events(&stdout);

        if !output.status.success() {
            let status = output
                .status
                .code()
                .map_or_else(|| "terminated by signal".into(), |code| format!("exit code {code}"));
            return Err(
                RuntimeError::new(format!("Codex failed with {status}")).with_diagnostics(events, stdout, stderr)
            );
        }

        let final_response = fs::read_to_string(&response_path).map_err(|error| {
            RuntimeError::new(format!("Codex did not produce a final response: {error}")).with_diagnostics(
                events.clone(),
                stdout.clone(),
                stderr.clone(),
            )
        })?;
        if final_response.trim().is_empty() {
            return Err(
                RuntimeError::new("Codex produced an empty final response").with_diagnostics(events, stdout, stderr)
            );
        }

        Ok(RuntimeResponse {
            output: final_response,
            events,
            stdout,
            stderr,
        })
    }
}

fn command_arguments(request: &RuntimeRequest, schema_path: &Path, response_path: &Path) -> Vec<OsString> {
    let mut arguments = vec![OsString::from("--ask-for-approval"), OsString::from("never")];
    match request.internet {
        Internet::Disabled => {
            arguments.push(OsString::from("--config"));
            arguments.push(OsString::from("web_search=\"disabled\""));
        },
        Internet::Enabled => arguments.push(OsString::from("--search")),
    }
    arguments.extend([
        OsString::from("exec"),
        OsString::from("--ephemeral"),
        OsString::from("--json"),
        OsString::from("--color"),
        OsString::from("never"),
        OsString::from("--sandbox"),
        OsString::from(match request.access {
            Access::ReadOnly => "read-only",
            Access::WorkspaceWrite => "workspace-write",
            Access::Full => "danger-full-access",
        }),
        OsString::from("--output-schema"),
        schema_path.as_os_str().to_owned(),
        OsString::from("--output-last-message"),
        response_path.as_os_str().to_owned(),
        OsString::from(&request.prompt),
    ]);
    arguments
}

fn parse_events(stdout: &str) -> Vec<Value> {
    stdout
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            serde_json::from_str(line).unwrap_or_else(|_| {
                json!({
                    "type": "brain.codex.unparsed_stdout",
                    "line": line,
                })
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use brain::{Access, Internet, RuntimeRequest};
    use serde_json::json;

    use super::*;

    fn request(access: Access, internet: Internet) -> RuntimeRequest {
        RuntimeRequest {
            flow_name: "investigate".into(),
            node_name: "research".into(),
            invocation: 1,
            prompt: "Research this".into(),
            output_schema: json!({ "type": "object" }),
            working_directory: PathBuf::from("."),
            access,
            internet,
        }
    }

    #[test]
    fn maps_basic_access_and_internet_settings_to_codex() {
        let arguments = command_arguments(
            &request(Access::WorkspaceWrite, Internet::Enabled),
            Path::new("schema.json"),
            Path::new("response.json"),
        );
        let arguments = arguments
            .iter()
            .map(|argument| argument.to_string_lossy().into_owned())
            .collect::<Vec<_>>();

        assert!(
            arguments
                .windows(2)
                .any(|pair| pair == ["--sandbox", "workspace-write"])
        );
        assert!(arguments.contains(&"--search".into()));
        assert!(arguments.contains(&"--ephemeral".into()));
        assert!(arguments.contains(&"--output-schema".into()));
    }

    #[test]
    fn disables_web_search_and_preserves_non_json_stdout() {
        let arguments = command_arguments(
            &request(Access::ReadOnly, Internet::Disabled),
            Path::new("schema.json"),
            Path::new("response.json"),
        );
        assert!(arguments.contains(&OsString::from("web_search=\"disabled\"")));

        let events = parse_events("{\"type\":\"turn.started\"}\nwarning\n");
        assert_eq!(events[0]["type"], "turn.started");
        assert_eq!(events[1]["type"], "brain.codex.unparsed_stdout");
    }
}
