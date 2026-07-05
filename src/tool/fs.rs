use crate::tool::{PermissionLevel, Tool, ToolContext, ToolOutcome};
use async_trait::async_trait;
use globset::{Glob, GlobSetBuilder};
use ignore::WalkBuilder;
use regex::Regex;
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};

pub struct ReadFileTool;
pub struct ListDirTool;
pub struct GlobTool;
pub struct GrepTool;

#[async_trait]
impl Tool for GrepTool {
    fn name(&self) -> &str {
        "grep"
    }

    fn description(&self) -> &str {
        "Search file contents with a regular expression."
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "pattern": { "type": "string" },
                "path": { "type": "string" }
            },
            "required": ["pattern"]
        })
    }

    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::ReadOnly
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> ToolOutcome {
        let Some(pattern) = string_arg(&args, "pattern") else {
            return error_outcome("missing or invalid pattern");
        };
        let path = string_arg(&args, "path").unwrap_or(".");
        let path = resolve_path(&ctx.cwd, path);
        if !path.is_dir() {
            return error_outcome(format!("directory not found: {}", path.display()));
        }

        let regex = match Regex::new(pattern) {
            Ok(regex) => regex,
            Err(err) => return error_outcome(format!("invalid regex: {err}")),
        };

        let mut matches = Vec::new();
        for item in walker(&path).build() {
            let item = match item {
                Ok(item) => item,
                Err(_) => continue,
            };
            if !item
                .file_type()
                .is_some_and(|file_type| file_type.is_file())
            {
                continue;
            }
            let entry_path = item.path();
            let Ok(relative) = entry_path.strip_prefix(&path) else {
                continue;
            };
            let relative = normalize_relative_path(relative);
            let content = match fs::read_to_string(entry_path) {
                Ok(content) => content,
                Err(_) => continue,
            };
            for (line_index, line) in content.lines().enumerate() {
                if regex.is_match(line) {
                    matches.push(format!("{}:{}:{}", relative, line_index + 1, line));
                }
            }
        }

        success_with_truncation(matches.join("\n"), ctx.max_output_bytes)
    }
}

#[async_trait]
impl Tool for GlobTool {
    fn name(&self) -> &str {
        "glob"
    }

    fn description(&self) -> &str {
        "Find files matching a glob pattern."
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "pattern": { "type": "string" },
                "path": { "type": "string" }
            },
            "required": ["pattern"]
        })
    }

    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::ReadOnly
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> ToolOutcome {
        let Some(pattern) = string_arg(&args, "pattern") else {
            return error_outcome("missing or invalid pattern");
        };
        let path = string_arg(&args, "path").unwrap_or(".");
        let path = resolve_path(&ctx.cwd, path);
        if !path.is_dir() {
            return error_outcome(format!("directory not found: {}", path.display()));
        }

        let glob = match Glob::new(pattern) {
            Ok(glob) => glob,
            Err(err) => return error_outcome(format!("invalid glob pattern: {err}")),
        };
        let mut builder = GlobSetBuilder::new();
        builder.add(glob);
        let matcher = match builder.build() {
            Ok(matcher) => matcher,
            Err(err) => return error_outcome(format!("invalid glob pattern: {err}")),
        };

        let mut entries = Vec::new();
        for item in walker(&path).build() {
            let item = match item {
                Ok(item) => item,
                Err(_) => continue,
            };
            if !item
                .file_type()
                .is_some_and(|file_type| file_type.is_file())
            {
                continue;
            }
            let entry_path = item.path();
            let Ok(relative) = entry_path.strip_prefix(&path) else {
                continue;
            };
            let relative = normalize_relative_path(relative);
            if matcher.is_match(&relative) {
                entries.push(relative);
            }
        }
        entries.sort();

        ToolOutcome {
            content: entries.join("\n"),
            is_error: false,
            truncated: false,
            exit: None,
        }
    }
}

#[async_trait]
impl Tool for ListDirTool {
    fn name(&self) -> &str {
        "list_dir"
    }

    fn description(&self) -> &str {
        "List directory entries while respecting gitignore rules."
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string" }
            }
        })
    }

    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::ReadOnly
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> ToolOutcome {
        let path = string_arg(&args, "path").unwrap_or(".");
        let path = resolve_path(&ctx.cwd, path);
        if !path.is_dir() {
            return error_outcome(format!("directory not found: {}", path.display()));
        }

        let mut entries = Vec::new();
        for item in walker(&path).max_depth(Some(1)).build() {
            let item = match item {
                Ok(item) => item,
                Err(err) => return error_outcome(format!("failed to list directory: {err}")),
            };
            let entry_path = item.path();
            if entry_path == path {
                continue;
            }
            let Ok(relative) = entry_path.strip_prefix(&path) else {
                continue;
            };
            entries.push(relative.display().to_string());
        }
        entries.sort();

        ToolOutcome {
            content: entries.join("\n"),
            is_error: false,
            truncated: false,
            exit: None,
        }
    }
}

#[async_trait]
impl Tool for ReadFileTool {
    fn name(&self) -> &str {
        "read_file"
    }

    fn description(&self) -> &str {
        "Read a file with optional line offset and limit."
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string" },
                "offset": { "type": "integer", "minimum": 0 },
                "limit": { "type": "integer", "minimum": 0 }
            },
            "required": ["path"]
        })
    }

    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::ReadOnly
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> ToolOutcome {
        let Some(path) = string_arg(&args, "path") else {
            return error_outcome("missing or invalid path");
        };
        let offset = usize_arg(&args, "offset").unwrap_or(0);
        let limit = usize_arg(&args, "limit");
        let path = resolve_path(&ctx.cwd, path);

        let content = match fs::read_to_string(&path) {
            Ok(content) => content,
            Err(err) => return error_outcome(format!("failed to read {}: {err}", path.display())),
        };
        let paged = page_lines(&content, offset, limit);

        success_with_truncation(paged, ctx.max_output_bytes)
    }
}

fn string_arg<'a>(args: &'a Value, field: &str) -> Option<&'a str> {
    args.get(field).and_then(Value::as_str)
}

fn usize_arg(args: &Value, field: &str) -> Option<usize> {
    args.get(field)
        .and_then(Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
}

fn resolve_path(cwd: &Path, path: &str) -> PathBuf {
    let path = PathBuf::from(path);
    if path.is_absolute() {
        path
    } else {
        cwd.join(path)
    }
}

fn normalize_relative_path(path: &Path) -> String {
    path.components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

fn walker(path: &Path) -> WalkBuilder {
    let mut builder = WalkBuilder::new(path);
    builder
        .git_global(false)
        .git_exclude(false)
        .require_git(false);
    builder
}

fn page_lines(content: &str, offset: usize, limit: Option<usize>) -> String {
    let lines = content.split_inclusive('\n').skip(offset);
    match limit {
        Some(limit) => lines.take(limit).collect(),
        None => lines.collect(),
    }
}

fn success_with_truncation(content: String, max_output_bytes: usize) -> ToolOutcome {
    let (content, truncated) = truncate_utf8(content, max_output_bytes);
    ToolOutcome {
        content,
        is_error: false,
        truncated,
        exit: None,
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

pub(crate) fn truncate_utf8(content: String, max_output_bytes: usize) -> (String, bool) {
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
    use super::{GlobTool, GrepTool, ListDirTool, ReadFileTool};
    use crate::tool::{PermissionLevel, Tool, ToolContext};
    use serde_json::json;
    use std::fs;

    fn ctx(root: &std::path::Path, max_output_bytes: usize) -> ToolContext {
        ToolContext {
            cwd: root.to_path_buf(),
            max_output_bytes,
        }
    }

    #[tokio::test]
    async fn read_file_reads_content_from_context_cwd() {
        let temp = tempfile::tempdir().unwrap();
        fs::write(temp.path().join("note.txt"), "alpha\nbeta\n").unwrap();
        let tool = ReadFileTool;

        let outcome = tool
            .execute(json!({ "path": "note.txt" }), &ctx(temp.path(), 4096))
            .await;

        assert_eq!(tool.name(), "read_file");
        assert_eq!(tool.permission_level(), PermissionLevel::ReadOnly);
        assert_eq!(outcome.content, "alpha\nbeta\n");
        assert!(!outcome.is_error);
        assert!(!outcome.truncated);
        assert_eq!(outcome.exit, None);
    }

    #[tokio::test]
    async fn read_file_pages_by_line_offset_and_limit() {
        let temp = tempfile::tempdir().unwrap();
        fs::write(temp.path().join("note.txt"), "l0\nl1\nl2\nl3\n").unwrap();
        let tool = ReadFileTool;

        let outcome = tool
            .execute(
                json!({ "path": "note.txt", "offset": 1, "limit": 2 }),
                &ctx(temp.path(), 4096),
            )
            .await;

        assert_eq!(outcome.content, "l1\nl2\n");
        assert!(!outcome.is_error);
        assert!(!outcome.truncated);
    }

    #[tokio::test]
    async fn read_file_truncates_on_utf8_character_boundary() {
        let temp = tempfile::tempdir().unwrap();
        fs::write(temp.path().join("note.txt"), "éééé").unwrap();
        let tool = ReadFileTool;

        let outcome = tool
            .execute(json!({ "path": "note.txt" }), &ctx(temp.path(), 5))
            .await;

        assert_eq!(outcome.content, "éé");
        assert!(!outcome.is_error);
        assert!(outcome.truncated);
    }

    #[tokio::test]
    async fn read_file_returns_error_for_missing_path() {
        let temp = tempfile::tempdir().unwrap();
        let tool = ReadFileTool;

        let outcome = tool
            .execute(json!({ "path": "missing.txt" }), &ctx(temp.path(), 4096))
            .await;

        assert!(outcome.is_error);
        assert!(!outcome.truncated);
    }

    #[tokio::test]
    async fn list_dir_lists_entries_and_respects_gitignore() {
        let temp = tempfile::tempdir().unwrap();
        fs::write(temp.path().join(".gitignore"), "ignored.txt\n").unwrap();
        fs::write(temp.path().join("visible.txt"), "ok").unwrap();
        fs::write(temp.path().join("ignored.txt"), "hidden").unwrap();
        let tool = ListDirTool;

        let outcome = tool
            .execute(json!({ "path": "." }), &ctx(temp.path(), 4096))
            .await;

        assert_eq!(tool.name(), "list_dir");
        assert_eq!(tool.permission_level(), PermissionLevel::ReadOnly);
        assert!(!outcome.is_error);
        assert_eq!(outcome.exit, None);
        assert!(outcome.content.contains("visible.txt"));
        assert!(!outcome.content.contains("ignored.txt"));
    }

    #[tokio::test]
    async fn list_dir_returns_error_for_missing_path() {
        let temp = tempfile::tempdir().unwrap();
        let tool = ListDirTool;

        let outcome = tool
            .execute(json!({ "path": "missing" }), &ctx(temp.path(), 4096))
            .await;

        assert!(outcome.is_error);
    }

    #[tokio::test]
    async fn glob_matches_files_from_context_cwd_and_respects_gitignore() {
        let temp = tempfile::tempdir().unwrap();
        fs::create_dir(temp.path().join("src")).unwrap();
        fs::write(temp.path().join(".gitignore"), "src/ignored.rs\n").unwrap();
        fs::write(temp.path().join("src").join("main.rs"), "fn main() {}").unwrap();
        fs::write(temp.path().join("src").join("notes.txt"), "notes").unwrap();
        fs::write(temp.path().join("src").join("ignored.rs"), "ignored").unwrap();
        let tool = GlobTool;

        let outcome = tool
            .execute(json!({ "pattern": "src/*.rs" }), &ctx(temp.path(), 4096))
            .await;

        assert_eq!(tool.name(), "glob");
        assert_eq!(tool.permission_level(), PermissionLevel::ReadOnly);
        assert!(!outcome.is_error);
        assert_eq!(outcome.exit, None);
        assert!(outcome.content.contains("src/main.rs"));
        assert!(!outcome.content.contains("src/notes.txt"));
        assert!(!outcome.content.contains("src/ignored.rs"));
    }

    #[tokio::test]
    async fn glob_returns_error_for_invalid_pattern() {
        let temp = tempfile::tempdir().unwrap();
        let tool = GlobTool;

        let outcome = tool
            .execute(json!({ "pattern": "[" }), &ctx(temp.path(), 4096))
            .await;

        assert!(outcome.is_error);
    }

    #[tokio::test]
    async fn grep_matches_regex_with_locations_and_respects_gitignore() {
        let temp = tempfile::tempdir().unwrap();
        fs::write(temp.path().join(".gitignore"), "ignored.txt\n").unwrap();
        fs::write(temp.path().join("visible.txt"), "alpha\nneedle beta\n").unwrap();
        fs::write(temp.path().join("ignored.txt"), "needle hidden\n").unwrap();
        let tool = GrepTool;

        let outcome = tool
            .execute(json!({ "pattern": "needle" }), &ctx(temp.path(), 4096))
            .await;

        assert_eq!(tool.name(), "grep");
        assert_eq!(tool.permission_level(), PermissionLevel::ReadOnly);
        assert!(!outcome.is_error);
        assert_eq!(outcome.exit, None);
        assert!(outcome.content.contains("visible.txt:2:needle beta"));
        assert!(!outcome.content.contains("ignored.txt"));
    }

    #[tokio::test]
    async fn grep_skips_non_utf8_files_and_returns_text_matches() {
        let temp = tempfile::tempdir().unwrap();
        fs::write(temp.path().join("binary.bin"), [0xff, 0xfe, 0xfd]).unwrap();
        fs::write(temp.path().join("visible.txt"), "needle beta\n").unwrap();
        let tool = GrepTool;

        let outcome = tool
            .execute(json!({ "pattern": "needle" }), &ctx(temp.path(), 4096))
            .await;

        assert!(!outcome.is_error);
        assert!(outcome.content.contains("visible.txt:1:needle beta"));
    }

    #[tokio::test]
    async fn grep_returns_error_for_invalid_regex() {
        let temp = tempfile::tempdir().unwrap();
        let tool = GrepTool;

        let outcome = tool
            .execute(json!({ "pattern": "(" }), &ctx(temp.path(), 4096))
            .await;

        assert!(outcome.is_error);
    }

    #[tokio::test]
    async fn grep_truncates_on_utf8_character_boundary() {
        let temp = tempfile::tempdir().unwrap();
        fs::write(temp.path().join("unicode.txt"), "éééé\n").unwrap();
        let tool = GrepTool;

        let outcome = tool
            .execute(json!({ "pattern": "é" }), &ctx(temp.path(), 16))
            .await;

        assert!(!outcome.is_error);
        assert!(outcome.truncated);
        assert!(outcome.content.len() <= 16);
    }
}
