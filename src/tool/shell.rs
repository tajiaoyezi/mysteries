use crate::tool::{PermissionLevel, Tool, ToolContext, ToolOutcome};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::process::Stdio;
use tokio::process::Command;
use tokio::time::{timeout, Duration};

pub struct RunShellTool;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[async_trait]
impl Tool for RunShellTool {
    fn name(&self) -> &str {
        "run_shell"
    }

    fn description(&self) -> &str {
        "Run a shell command with timeout and captured output."
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "command": { "type": "string" },
                "timeout_secs": { "type": "integer", "minimum": 1 }
            },
            "required": ["command"]
        })
    }

    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::Execute
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> ToolOutcome {
        let Some(command) = string_arg(&args, "command") else {
            return error_outcome("missing or invalid command");
        };
        let timeout_secs = match usize_arg(&args, "timeout_secs") {
            Some(0) => return error_outcome("timeout_secs must be greater than 0"),
            Some(value) => value,
            None => 30,
        };

        let mut child = platform_command(command);
        child
            .current_dir(&ctx.cwd)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);

        let child = match child.spawn() {
            Ok(child) => child,
            Err(err) => return error_outcome(format!("failed to spawn command: {err}")),
        };

        let output = match timeout(
            Duration::from_secs(timeout_secs as u64),
            child.wait_with_output(),
        )
        .await
        {
            Ok(Ok(output)) => output,
            Ok(Err(err)) => return error_outcome(format!("failed to wait for command: {err}")),
            Err(_) => return error_outcome(format!("command timed out after {timeout_secs}s")),
        };

        let exit = output.status.code();
        let code = exit.map_or_else(|| "signal".to_string(), |code| code.to_string());
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        let content = format!("exit: {code}\n--- stdout ---\n{stdout}\n--- stderr ---\n{stderr}");
        let is_error = !output.status.success();

        outcome_with_truncation(content, is_error, ctx.max_output_bytes, exit)
    }
}

#[cfg(windows)]
fn platform_command(command: &str) -> Command {
    let mut cmd = Command::new("cmd");
    cmd.arg("/C").arg(command);
    // 避免子进程 attach 当前 console 后重置 TUI 已启用的输入模式(如鼠标捕获)。
    cmd.creation_flags(CREATE_NO_WINDOW);
    cmd
}

#[cfg(not(windows))]
fn platform_command(command: &str) -> Command {
    let mut cmd = Command::new("sh");
    cmd.arg("-c").arg(command);
    cmd
}

fn string_arg<'a>(args: &'a Value, field: &str) -> Option<&'a str> {
    args.get(field).and_then(Value::as_str)
}

fn usize_arg(args: &Value, field: &str) -> Option<usize> {
    args.get(field)
        .and_then(Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
}

fn outcome_with_truncation(
    content: String,
    is_error: bool,
    max_output_bytes: usize,
    exit: Option<i32>,
) -> ToolOutcome {
    let (content, truncated) = truncate_utf8(content, max_output_bytes);
    ToolOutcome {
        content,
        is_error,
        truncated,
        exit,
    }
}

fn error_outcome(content: impl Into<String>) -> ToolOutcome {
    ToolOutcome {
        content: content.into(),
        is_error: true,
        truncated: false,
        exit: None,
    }
}

fn truncate_utf8(content: String, max_output_bytes: usize) -> (String, bool) {
    if content.len() <= max_output_bytes {
        return (content, false);
    }

    let mut boundary = 0;
    for (index, _) in content.char_indices() {
        if index <= max_output_bytes {
            boundary = index;
        } else {
            break;
        }
    }
    if content.is_char_boundary(max_output_bytes) {
        boundary = max_output_bytes;
    }

    (content[..boundary].to_string(), true)
}

#[cfg(test)]
mod tests {
    use super::RunShellTool;
    use crate::tool::{PermissionLevel, Tool, ToolContext};
    use serde_json::json;
    use std::path::Path;

    fn ctx(root: &Path, max_output_bytes: usize) -> ToolContext {
        ToolContext {
            cwd: root.to_path_buf(),
            max_output_bytes,
        }
    }

    #[cfg(windows)]
    fn success_command() -> &'static str {
        "echo shell-out && echo shell-err 1>&2"
    }

    #[cfg(not(windows))]
    fn success_command() -> &'static str {
        "printf 'shell-out\\n'; printf 'shell-err\\n' >&2"
    }

    #[cfg(windows)]
    fn nonzero_command() -> &'static str {
        "echo failed && exit /b 7"
    }

    #[cfg(not(windows))]
    fn nonzero_command() -> &'static str {
        "printf 'failed\\n'; exit 7"
    }

    #[cfg(windows)]
    fn timeout_command() -> &'static str {
        "ping -n 3 127.0.0.1 > nul"
    }

    #[cfg(not(windows))]
    fn timeout_command() -> &'static str {
        "sleep 5"
    }

    #[cfg(windows)]
    fn unicode_command() -> &'static str {
        "echo éééé"
    }

    #[cfg(not(windows))]
    fn unicode_command() -> &'static str {
        "printf 'éééé\\n'"
    }

    #[tokio::test]
    async fn run_shell_captures_stdout_stderr_and_exit_code() {
        let temp = tempfile::tempdir().unwrap();
        let tool = RunShellTool;

        let outcome = tool
            .execute(
                json!({ "command": success_command(), "timeout_secs": 5 }),
                &ctx(temp.path(), 4096),
            )
            .await;

        assert_eq!(tool.name(), "run_shell");
        assert_eq!(
            tool.permission_level(),
            PermissionLevel::Execute
        );
        assert!(!outcome.is_error);
        assert_eq!(outcome.exit, Some(0));
        assert!(outcome.content.contains("exit: 0"));
        assert!(outcome.content.contains("shell-out"));
        assert!(outcome.content.contains("shell-err"));
    }

    #[tokio::test]
    async fn run_shell_returns_error_for_nonzero_exit() {
        let temp = tempfile::tempdir().unwrap();
        let tool = RunShellTool;

        let outcome = tool
            .execute(
                json!({ "command": nonzero_command(), "timeout_secs": 5 }),
                &ctx(temp.path(), 4096),
            )
            .await;

        assert!(outcome.is_error);
        assert_eq!(outcome.exit, Some(7));
        assert!(outcome.content.contains("exit: 7"));
        assert!(outcome.content.contains("failed"));
    }

    #[tokio::test]
    async fn run_shell_returns_error_when_timeout_is_hit() {
        let temp = tempfile::tempdir().unwrap();
        let tool = RunShellTool;

        let outcome = tool
            .execute(
                json!({ "command": timeout_command(), "timeout_secs": 1 }),
                &ctx(temp.path(), 4096),
            )
            .await;

        assert!(outcome.is_error);
        assert!(outcome.content.contains("timed out"));
    }

    #[tokio::test]
    async fn run_shell_truncates_output_on_utf8_character_boundary() {
        let temp = tempfile::tempdir().unwrap();
        let tool = RunShellTool;

        let outcome = tool
            .execute(
                json!({ "command": unicode_command(), "timeout_secs": 5 }),
                &ctx(temp.path(), 25),
            )
            .await;

        assert!(outcome.truncated);
        assert!(outcome.content.len() <= 25);
    }
}
