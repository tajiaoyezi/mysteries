use crate::tool::{PermissionLevel, Tool, ToolContext, ToolOutcome};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};

pub struct WriteFileTool;
pub struct EditFileTool;

#[async_trait]
impl Tool for EditFileTool {
    fn name(&self) -> &str {
        "edit_file"
    }

    fn description(&self) -> &str {
        "Replace a unique string in a file."
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string" },
                "old_string": { "type": "string" },
                "new_string": { "type": "string" }
            },
            "required": ["path", "old_string", "new_string"]
        })
    }

    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::Edit
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> ToolOutcome {
        let Some(path) = string_arg(&args, "path") else {
            return error_outcome("missing or invalid path");
        };
        let Some(old_string) = string_arg(&args, "old_string") else {
            return error_outcome("missing or invalid old_string");
        };
        let Some(new_string) = string_arg(&args, "new_string") else {
            return error_outcome("missing or invalid new_string");
        };
        let path = resolve_path(&ctx.cwd, path);

        let content = match fs::read_to_string(&path) {
            Ok(content) => content,
            Err(err) => return error_outcome(format!("failed to read {}: {err}", path.display())),
        };
        let match_count = content.matches(old_string).count();
        if match_count != 1 {
            return error_outcome(format!("expected exactly one match, found {match_count}"));
        }

        let updated = content.replace(old_string, new_string);
        match fs::write(&path, updated) {
            Ok(()) => ToolOutcome {
                content: format!("edited {}", path.display()),
                is_error: false,
                truncated: false,
                exit: None,
            },
            Err(err) => error_outcome(format!("failed to write {}: {err}", path.display())),
        }
    }
}

#[async_trait]
impl Tool for WriteFileTool {
    fn name(&self) -> &str {
        "write_file"
    }

    fn description(&self) -> &str {
        "Create or overwrite a file."
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string" },
                "content": { "type": "string" }
            },
            "required": ["path", "content"]
        })
    }

    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::Edit
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> ToolOutcome {
        let Some(path) = string_arg(&args, "path") else {
            return error_outcome("missing or invalid path");
        };
        let Some(content) = string_arg(&args, "content") else {
            return error_outcome("missing or invalid content");
        };
        let path = resolve_path(&ctx.cwd, path);

        match fs::write(&path, content) {
            Ok(()) => ToolOutcome {
                content: format!("wrote {}", path.display()),
                is_error: false,
                truncated: false,
                exit: None,
            },
            Err(err) => error_outcome(format!("failed to write {}: {err}", path.display())),
        }
    }
}

fn string_arg<'a>(args: &'a Value, field: &str) -> Option<&'a str> {
    args.get(field).and_then(Value::as_str)
}

fn resolve_path(cwd: &Path, path: &str) -> PathBuf {
    let path = PathBuf::from(path);
    if path.is_absolute() {
        path
    } else {
        cwd.join(path)
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

#[cfg(test)]
mod tests {
    use super::{EditFileTool, WriteFileTool};
    use crate::tool::{PermissionLevel, Tool, ToolContext};
    use serde_json::json;
    use std::fs;

    fn ctx(root: &std::path::Path) -> ToolContext {
        ToolContext {
            cwd: root.to_path_buf(),
            max_output_bytes: 4096,
        }
    }

    #[tokio::test]
    async fn write_file_creates_new_file() {
        let temp = tempfile::tempdir().unwrap();
        let tool = WriteFileTool;

        let outcome = tool
            .execute(
                json!({ "path": "new.txt", "content": "hello" }),
                &ctx(temp.path()),
            )
            .await;

        assert_eq!(tool.name(), "write_file");
        assert_eq!(
            tool.permission_level(),
            PermissionLevel::Edit
        );
        assert!(!outcome.is_error);
        assert_eq!(outcome.exit, None);
        assert_eq!(
            fs::read_to_string(temp.path().join("new.txt")).unwrap(),
            "hello"
        );
    }

    #[tokio::test]
    async fn write_file_overwrites_existing_file() {
        let temp = tempfile::tempdir().unwrap();
        fs::write(temp.path().join("note.txt"), "old").unwrap();
        let tool = WriteFileTool;

        let outcome = tool
            .execute(
                json!({ "path": "note.txt", "content": "new" }),
                &ctx(temp.path()),
            )
            .await;

        assert!(!outcome.is_error);
        assert_eq!(outcome.exit, None);
        assert_eq!(
            fs::read_to_string(temp.path().join("note.txt")).unwrap(),
            "new"
        );
    }

    #[tokio::test]
    async fn write_file_returns_error_when_parent_directory_is_missing() {
        let temp = tempfile::tempdir().unwrap();
        let tool = WriteFileTool;

        let outcome = tool
            .execute(
                json!({ "path": "missing/new.txt", "content": "hello" }),
                &ctx(temp.path()),
            )
            .await;

        assert!(outcome.is_error);
        assert!(!temp.path().join("missing").exists());
    }

    #[tokio::test]
    async fn write_file_returns_error_when_target_cannot_be_written() {
        let temp = tempfile::tempdir().unwrap();
        fs::create_dir(temp.path().join("dir.txt")).unwrap();
        let tool = WriteFileTool;

        let outcome = tool
            .execute(
                json!({ "path": "dir.txt", "content": "hello" }),
                &ctx(temp.path()),
            )
            .await;

        assert!(outcome.is_error);
    }

    #[tokio::test]
    async fn edit_file_replaces_unique_match() {
        let temp = tempfile::tempdir().unwrap();
        fs::write(temp.path().join("note.txt"), "alpha beta gamma").unwrap();
        let tool = EditFileTool;

        let outcome = tool
            .execute(
                json!({
                    "path": "note.txt",
                    "old_string": "beta",
                    "new_string": "delta"
                }),
                &ctx(temp.path()),
            )
            .await;

        assert_eq!(tool.name(), "edit_file");
        assert_eq!(
            tool.permission_level(),
            PermissionLevel::Edit
        );
        assert!(!outcome.is_error);
        assert_eq!(outcome.exit, None);
        assert_eq!(
            fs::read_to_string(temp.path().join("note.txt")).unwrap(),
            "alpha delta gamma"
        );
    }

    #[tokio::test]
    async fn edit_file_returns_error_and_preserves_file_when_match_is_missing() {
        let temp = tempfile::tempdir().unwrap();
        fs::write(temp.path().join("note.txt"), "alpha beta gamma").unwrap();
        let tool = EditFileTool;

        let outcome = tool
            .execute(
                json!({
                    "path": "note.txt",
                    "old_string": "missing",
                    "new_string": "delta"
                }),
                &ctx(temp.path()),
            )
            .await;

        assert!(outcome.is_error);
        assert_eq!(
            fs::read_to_string(temp.path().join("note.txt")).unwrap(),
            "alpha beta gamma"
        );
    }

    #[tokio::test]
    async fn edit_file_returns_error_and_preserves_file_when_match_is_not_unique() {
        let temp = tempfile::tempdir().unwrap();
        fs::write(temp.path().join("note.txt"), "alpha beta beta").unwrap();
        let tool = EditFileTool;

        let outcome = tool
            .execute(
                json!({
                    "path": "note.txt",
                    "old_string": "beta",
                    "new_string": "delta"
                }),
                &ctx(temp.path()),
            )
            .await;

        assert!(outcome.is_error);
        assert_eq!(
            fs::read_to_string(temp.path().join("note.txt")).unwrap(),
            "alpha beta beta"
        );
    }
}
