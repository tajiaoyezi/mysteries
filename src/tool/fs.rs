use crate::tool::{
    process_blocking_limiter, run_blocking_tool, PermissionLevel, Tool, ToolConcurrency,
    ToolContext, ToolExecutionContext, ToolOutcome,
};
use async_trait::async_trait;
use globset::{Glob, GlobSetBuilder};
use ignore::{gitignore::Gitignore, DirEntry, WalkBuilder};
use regex::Regex;
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

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

    fn concurrency(&self) -> ToolConcurrency {
        ToolConcurrency::ParallelSafe
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> ToolOutcome {
        let ctx = ctx.clone();
        run_blocking_tool(&process_blocking_limiter(), move || {
            execute_grep(args, &ctx)
        })
        .await
    }

    async fn execute_scoped(&self, args: Value, ctx: &ToolExecutionContext<'_>) -> ToolOutcome {
        let Some(read_root) = ctx.read_root else {
            return self.execute(args, ctx.tool).await;
        };
        if string_arg(&args, "pattern").is_none() {
            return error_outcome("missing or invalid pattern");
        }
        let path = string_arg(&args, "path").unwrap_or(".");
        let unresolved_target = resolve_path(&ctx.tool.cwd, path);
        let read_root = read_root.to_path_buf();
        let tool_context = ctx.tool.clone();
        run_contained_fs(
            read_root,
            unresolved_target,
            move |canonical_root, canonical_target| {
                execute_grep_at(
                    &args,
                    &tool_context,
                    &canonical_target,
                    Some(&canonical_root),
                )
            },
        )
        .await
    }
}

fn execute_grep(args: Value, ctx: &ToolContext) -> ToolOutcome {
    let path = string_arg(&args, "path").unwrap_or(".");
    let path = resolve_path(&ctx.cwd, path);
    execute_grep_at(&args, ctx, &path, None)
}

fn execute_grep_at(
    args: &Value,
    ctx: &ToolContext,
    path: &Path,
    read_root: Option<&Path>,
) -> ToolOutcome {
    let Some(pattern) = string_arg(args, "pattern") else {
        return error_outcome("missing or invalid pattern");
    };
    if !path.is_dir() {
        return error_outcome(format!("directory not found: {}", path.display()));
    }

    let regex = match Regex::new(pattern) {
        Ok(regex) => regex,
        Err(err) => return error_outcome(format!("invalid regex: {err}")),
    };

    let walk = match directory_walker(path, read_root, None) {
        Ok(walk) => walk,
        Err(outcome) => return outcome,
    };
    #[cfg(test)]
    test_probe::record_target_io(path);
    let mut matches = Vec::new();
    for item in walk.build() {
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
        let Ok(relative) = entry_path.strip_prefix(path) else {
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

    fn concurrency(&self) -> ToolConcurrency {
        ToolConcurrency::ParallelSafe
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> ToolOutcome {
        let ctx = ctx.clone();
        run_blocking_tool(&process_blocking_limiter(), move || {
            execute_glob(args, &ctx)
        })
        .await
    }

    async fn execute_scoped(&self, args: Value, ctx: &ToolExecutionContext<'_>) -> ToolOutcome {
        let Some(read_root) = ctx.read_root else {
            return self.execute(args, ctx.tool).await;
        };
        if string_arg(&args, "pattern").is_none() {
            return error_outcome("missing or invalid pattern");
        }
        let path = string_arg(&args, "path").unwrap_or(".");
        let unresolved_target = resolve_path(&ctx.tool.cwd, path);
        let read_root = read_root.to_path_buf();
        run_contained_fs(
            read_root,
            unresolved_target,
            move |canonical_root, canonical_target| {
                execute_glob_at(&args, &canonical_target, Some(&canonical_root))
            },
        )
        .await
    }
}

fn execute_glob(args: Value, ctx: &ToolContext) -> ToolOutcome {
    let path = string_arg(&args, "path").unwrap_or(".");
    let path = resolve_path(&ctx.cwd, path);
    execute_glob_at(&args, &path, None)
}

fn execute_glob_at(args: &Value, path: &Path, read_root: Option<&Path>) -> ToolOutcome {
    let Some(pattern) = string_arg(args, "pattern") else {
        return error_outcome("missing or invalid pattern");
    };
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

    let walk = match directory_walker(path, read_root, None) {
        Ok(walk) => walk,
        Err(outcome) => return outcome,
    };
    #[cfg(test)]
    test_probe::record_target_io(path);
    let mut entries = Vec::new();
    for item in walk.build() {
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
        let Ok(relative) = entry_path.strip_prefix(path) else {
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

    fn concurrency(&self) -> ToolConcurrency {
        ToolConcurrency::ParallelSafe
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> ToolOutcome {
        let ctx = ctx.clone();
        run_blocking_tool(&process_blocking_limiter(), move || {
            execute_list_dir(args, &ctx)
        })
        .await
    }

    async fn execute_scoped(&self, args: Value, ctx: &ToolExecutionContext<'_>) -> ToolOutcome {
        let Some(read_root) = ctx.read_root else {
            return self.execute(args, ctx.tool).await;
        };
        let path = string_arg(&args, "path").unwrap_or(".");
        let unresolved_target = resolve_path(&ctx.tool.cwd, path);
        let read_root = read_root.to_path_buf();
        run_contained_fs(
            read_root,
            unresolved_target,
            move |canonical_root, canonical_target| {
                execute_list_dir_at(&canonical_target, Some(&canonical_root))
            },
        )
        .await
    }
}

fn execute_list_dir(args: Value, ctx: &ToolContext) -> ToolOutcome {
    let path = string_arg(&args, "path").unwrap_or(".");
    let path = resolve_path(&ctx.cwd, path);
    execute_list_dir_at(&path, None)
}

fn execute_list_dir_at(path: &Path, read_root: Option<&Path>) -> ToolOutcome {
    if !path.is_dir() {
        return error_outcome(format!("directory not found: {}", path.display()));
    }

    let walk = match directory_walker(path, read_root, Some(1)) {
        Ok(walk) => walk,
        Err(outcome) => return outcome,
    };
    #[cfg(test)]
    test_probe::record_target_io(path);
    let mut entries = Vec::new();
    for item in walk.build() {
        let item = match item {
            Ok(item) => item,
            Err(err) => return error_outcome(format!("failed to list directory: {err}")),
        };
        let entry_path = item.path();
        if entry_path == path {
            continue;
        }
        let Ok(relative) = entry_path.strip_prefix(path) else {
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

    fn concurrency(&self) -> ToolConcurrency {
        ToolConcurrency::ParallelSafe
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> ToolOutcome {
        let ctx = ctx.clone();
        run_blocking_tool(&process_blocking_limiter(), move || {
            execute_read_file(args, &ctx)
        })
        .await
    }

    async fn execute_scoped(&self, args: Value, ctx: &ToolExecutionContext<'_>) -> ToolOutcome {
        let Some(read_root) = ctx.read_root else {
            return self.execute(args, ctx.tool).await;
        };
        let Some(path) = string_arg(&args, "path") else {
            return error_outcome("missing or invalid path");
        };
        let unresolved_target = resolve_path(&ctx.tool.cwd, path);
        let read_root = read_root.to_path_buf();
        let tool_context = ctx.tool.clone();
        run_contained_fs(
            read_root,
            unresolved_target,
            move |_canonical_root, canonical_target| {
                execute_read_file_at(&args, &tool_context, &canonical_target)
            },
        )
        .await
    }
}

fn execute_read_file(args: Value, ctx: &ToolContext) -> ToolOutcome {
    let Some(path) = string_arg(&args, "path") else {
        return error_outcome("missing or invalid path");
    };
    let path = resolve_path(&ctx.cwd, path);
    execute_read_file_at(&args, ctx, &path)
}

fn execute_read_file_at(args: &Value, ctx: &ToolContext, path: &Path) -> ToolOutcome {
    let offset = usize_arg(args, "offset").unwrap_or(0);
    let limit = usize_arg(args, "limit");
    #[cfg(test)]
    test_probe::record_target_io(path);
    let content = match fs::read_to_string(path) {
        Ok(content) => content,
        Err(err) => return error_outcome(format!("failed to read {}: {err}", path.display())),
    };
    let paged = page_lines(&content, offset, limit);

    success_with_truncation(paged, ctx.max_output_bytes)
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

async fn run_contained_fs<F>(read_root: PathBuf, unresolved_target: PathBuf, work: F) -> ToolOutcome
where
    F: FnOnce(PathBuf, PathBuf) -> ToolOutcome + Send + 'static,
{
    #[cfg(test)]
    let invocation_id = test_probe::next_invocation_id();
    run_blocking_tool(&process_blocking_limiter(), move || {
        let canonical_root = match fs::canonicalize(&read_root) {
            Ok(path) => path,
            Err(err) => {
                return error_outcome(format!("failed to canonicalize read root: {err}"));
            }
        };
        if !canonical_root.is_dir() {
            return error_outcome("read root is not a directory");
        }

        #[cfg(test)]
        test_probe::record_canonicalize(&unresolved_target, invocation_id);
        let canonical_target = match fs::canonicalize(&unresolved_target) {
            Ok(path) => path,
            Err(err) => return error_outcome(format!("failed to canonicalize path: {err}")),
        };
        if canonical_target != canonical_root && !canonical_target.starts_with(&canonical_root) {
            return error_outcome("path escapes read root");
        }

        #[cfg(test)]
        let _invocation = test_probe::enter_invocation(invocation_id);
        work(canonical_root, canonical_target)
    })
    .await
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
        .require_git(false)
        .follow_links(false);
    builder
}

fn directory_walker(
    path: &Path,
    read_root: Option<&Path>,
    max_target_depth: Option<usize>,
) -> Result<WalkBuilder, ToolOutcome> {
    let Some(read_root) = read_root else {
        let mut builder = walker(path);
        builder.max_depth(max_target_depth);
        return Ok(builder);
    };

    let ignore_rules = scoped_ignore_rules(read_root, path, max_target_depth)?;
    let target = path.to_path_buf();
    let mut builder = walker(path);
    // Start at the explicit target so an ancestor rule cannot prune the
    // caller-selected root. Parent discovery stays disabled; the bounded,
    // prevalidated matchers reproduce in-root ancestor and nested rules.
    builder
        .parents(false)
        .ignore(false)
        .git_ignore(false)
        .filter_entry(move |entry| {
            if entry.path() == target {
                return true;
            }
            !ignore_rules.is_ignored(
                entry.path(),
                entry
                    .file_type()
                    .is_some_and(|file_type| file_type.is_dir()),
            )
        });
    builder.max_depth(max_target_depth);
    Ok(builder)
}

#[derive(Clone, Default)]
struct ScopedIgnoreRules {
    ignore: Vec<Gitignore>,
    git_ignore: Vec<Gitignore>,
}

impl ScopedIgnoreRules {
    fn load_directory(&mut self, read_root: &Path, directory: &Path) -> Result<(), ToolOutcome> {
        for (name, rules) in [
            (".ignore", &mut self.ignore),
            (".gitignore", &mut self.git_ignore),
        ] {
            let candidate = directory.join(name);
            #[cfg(test)]
            test_probe::record_ignore_control(&candidate);
            match fs::symlink_metadata(&candidate) {
                Ok(_) => {}
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => continue,
                Err(_) => return Err(error_outcome("failed to validate ignore file")),
            }
            let canonical = fs::canonicalize(&candidate)
                .map_err(|_| error_outcome("failed to validate ignore file"))?;
            if canonical != read_root && !canonical.starts_with(read_root) {
                return Err(error_outcome("path escapes read root"));
            }

            // Gitignore::new preserves valid rules when a file is only
            // partially parseable, matching the walker's existing behavior.
            let (matcher, _) = Gitignore::new(&candidate);
            rules.push(matcher);
        }
        Ok(())
    }

    fn is_ignored(&self, path: &Path, is_dir: bool) -> bool {
        rule_decision(&self.ignore, path, is_dir)
            .or_else(|| rule_decision(&self.git_ignore, path, is_dir))
            .unwrap_or(false)
    }
}

fn rule_decision(rules: &[Gitignore], path: &Path, is_dir: bool) -> Option<bool> {
    for matcher in rules.iter().rev() {
        let base = matcher.path();
        if path == base || !path.starts_with(base) {
            continue;
        }
        let matched = matcher.matched(path, is_dir);
        if matched.is_ignore() {
            return Some(true);
        }
        if matched.is_whitelist() {
            return Some(false);
        }
    }
    None
}

fn scoped_ignore_rules(
    read_root: &Path,
    target: &Path,
    max_target_depth: Option<usize>,
) -> Result<ScopedIgnoreRules, ToolOutcome> {
    let relative_target = target
        .strip_prefix(read_root)
        .map_err(|_| error_outcome("path escapes read root"))?;

    let mut rules = ScopedIgnoreRules::default();
    let mut ancestor = read_root.to_path_buf();
    for component in relative_target.components() {
        rules.load_directory(read_root, &ancestor)?;
        ancestor.push(component.as_os_str());
    }

    rules.load_directory(read_root, target)?;
    let shared_rules = Arc::new(Mutex::new(rules));
    let validation_error = Arc::new(Mutex::new(None));
    {
        let filter_rules = Arc::clone(&shared_rules);
        let filter_error = Arc::clone(&validation_error);
        let read_root = read_root.to_path_buf();
        let mut validation = WalkBuilder::new(target);
        validation
            .standard_filters(false)
            .follow_links(false)
            .max_depth(max_target_depth)
            .filter_entry(move |entry| {
                let is_directory = entry
                    .file_type()
                    .is_some_and(|file_type| file_type.is_dir());
                if !is_directory {
                    return true;
                }
                if filter_error.lock().unwrap().is_some() {
                    return false;
                }

                let mut rules = filter_rules.lock().unwrap();
                let pruned = scoped_entry_is_hidden(entry) || rules.is_ignored(entry.path(), true);
                if let Err(error) = rules.load_directory(&read_root, entry.path()) {
                    *filter_error.lock().unwrap() = Some(error);
                    return false;
                }
                !pruned
            });
        for item in validation.build() {
            item.map_err(|_| error_outcome("failed to validate ignore files"))?;
        }
    }
    if let Some(error) = validation_error.lock().unwrap().take() {
        return Err(error);
    }
    let rules = shared_rules.lock().unwrap().clone();
    Ok(rules)
}

fn scoped_entry_is_hidden(entry: &DirEntry) -> bool {
    if entry.file_name().to_string_lossy().starts_with('.') {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;

        const FILE_ATTRIBUTE_HIDDEN: u32 = 0x2;
        if let Ok(metadata) = entry.metadata() {
            return metadata.file_attributes() & FILE_ATTRIBUTE_HIDDEN != 0;
        }
    }
    false
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
mod test_probe {
    use std::cell::Cell;
    use std::collections::HashMap;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::{Arc, LazyLock, Mutex};
    use std::thread::ThreadId;

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub(super) enum FsProbeStage {
        Canonicalize,
        IgnoreControl,
        TargetIo,
    }

    #[derive(Clone, Debug, PartialEq, Eq)]
    pub(super) struct FsProbeEvent {
        pub(super) stage: FsProbeStage,
        pub(super) thread: ThreadId,
        pub(super) invocation_id: Option<u64>,
    }

    type ProbeEvents = Arc<Mutex<Vec<FsProbeEvent>>>;
    type ProbeRegistry = Mutex<HashMap<PathBuf, ProbeEvents>>;

    static PROBES: LazyLock<ProbeRegistry> = LazyLock::new(|| Mutex::new(HashMap::new()));
    static NEXT_INVOCATION_ID: AtomicU64 = AtomicU64::new(1);
    thread_local! {
        static ACTIVE_INVOCATION_ID: Cell<Option<u64>> = const { Cell::new(None) };
    }

    pub(super) struct FsProbe {
        paths: Vec<PathBuf>,
        events: ProbeEvents,
    }

    impl FsProbe {
        pub(super) fn install(path: PathBuf) -> Self {
            let mut paths = vec![path.clone()];
            if let Ok(canonical) = std::fs::canonicalize(&path) {
                if canonical != path {
                    paths.push(canonical);
                }
            }
            let events = Arc::new(Mutex::new(Vec::new()));
            let mut probes = PROBES.lock().unwrap();
            for alias in &paths {
                assert!(
                    !probes.contains_key(alias),
                    "a filesystem probe is already installed for {}",
                    alias.display()
                );
            }
            for alias in &paths {
                probes.insert(alias.clone(), events.clone());
            }
            drop(probes);
            Self { paths, events }
        }

        pub(super) fn events(&self) -> Vec<FsProbeEvent> {
            self.events.lock().unwrap().clone()
        }

        pub(super) fn target_io_count(&self) -> usize {
            self.events()
                .iter()
                .filter(|event| event.stage == FsProbeStage::TargetIo)
                .count()
        }

        pub(super) fn ignore_control_count(&self) -> usize {
            self.events()
                .iter()
                .filter(|event| event.stage == FsProbeStage::IgnoreControl)
                .count()
        }
    }

    impl Drop for FsProbe {
        fn drop(&mut self) {
            let mut probes = PROBES.lock().unwrap();
            for path in &self.paths {
                probes.remove(path);
            }
        }
    }

    pub(super) struct InvocationGuard {
        previous: Option<u64>,
    }

    impl Drop for InvocationGuard {
        fn drop(&mut self) {
            ACTIVE_INVOCATION_ID.with(|active| active.set(self.previous));
        }
    }

    pub(super) fn next_invocation_id() -> u64 {
        NEXT_INVOCATION_ID.fetch_add(1, Ordering::Relaxed)
    }

    pub(super) fn enter_invocation(invocation_id: u64) -> InvocationGuard {
        let previous = ACTIVE_INVOCATION_ID.with(|active| active.replace(Some(invocation_id)));
        InvocationGuard { previous }
    }

    pub(super) fn record_canonicalize(path: &Path, invocation_id: u64) {
        record(path, FsProbeStage::Canonicalize, Some(invocation_id));
    }

    pub(super) fn record_target_io(path: &Path) {
        let invocation_id = ACTIVE_INVOCATION_ID.with(Cell::get);
        record(path, FsProbeStage::TargetIo, invocation_id);
    }

    pub(super) fn record_ignore_control(path: &Path) {
        let invocation_id = ACTIVE_INVOCATION_ID.with(Cell::get);
        record(path, FsProbeStage::IgnoreControl, invocation_id);
    }

    fn record(path: &Path, stage: FsProbeStage, invocation_id: Option<u64>) {
        let events = PROBES.lock().unwrap().get(path).cloned();
        if let Some(events) = events {
            events.lock().unwrap().push(FsProbeEvent {
                stage,
                thread: std::thread::current().id(),
                invocation_id,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        test_probe::{FsProbe, FsProbeStage},
        GlobTool, GrepTool, ListDirTool, ReadFileTool,
    };
    use crate::agent::{AgentExecutionScope, ExecutionBudget, ExecutionCapabilities, NoopObserver};
    use crate::tool::{
        PermissionLevel, Tool, ToolConcurrency, ToolContext, ToolExecutionContext, ToolOutcome,
    };
    use serde_json::json;
    use std::fs;
    use std::io;
    use std::path::{Path, PathBuf};
    use std::time::Duration;

    const EXTERNAL_SECRET_MARKER: &str = "SCOPED_EXTERNAL_MARKER_7D89F831";

    fn ctx(root: &std::path::Path, max_output_bytes: usize) -> ToolContext {
        ToolContext {
            cwd: root.to_path_buf(),
            max_output_bytes,
        }
    }

    fn path_arg(path: &Path) -> String {
        path.to_string_lossy().into_owned()
    }

    fn directory_args(tool: &dyn Tool, path: impl Into<String>) -> serde_json::Value {
        let path = path.into();
        match tool.name() {
            "list_dir" => json!({ "path": path }),
            "glob" => json!({ "path": path, "pattern": "*.txt" }),
            "grep" => json!({ "path": path, "pattern": "SCOPED_NEEDLE" }),
            other => panic!("unexpected directory tool: {other}"),
        }
    }

    async fn execute_scoped(
        tool: &dyn Tool,
        args: serde_json::Value,
        tool_context: &ToolContext,
        read_root: Option<&Path>,
    ) -> ToolOutcome {
        let scope = AgentExecutionScope::root(
            ExecutionBudget::new(8, None, 0),
            ExecutionCapabilities::try_new([tool.name()], [PermissionLevel::ReadOnly]).unwrap(),
        );
        let observer = NoopObserver;
        tool.execute_scoped(
            args,
            &ToolExecutionContext {
                tool: tool_context,
                scope: &scope,
                observer: &observer,
                read_root,
            },
        )
        .await
    }

    struct ContainmentFixture {
        _temp: tempfile::TempDir,
        workspace: PathBuf,
        canonical_workspace: PathBuf,
        outside: PathBuf,
        marker_file: PathBuf,
    }

    impl ContainmentFixture {
        fn new() -> Self {
            let temp = tempfile::tempdir().unwrap();
            let workspace = temp.path().join("workspace");
            let outside = temp.path().join("workspace-escape");
            fs::create_dir(&workspace).unwrap();
            fs::create_dir(&outside).unwrap();
            fs::create_dir(workspace.join("inside")).unwrap();
            fs::write(
                workspace.join("inside").join("visible.txt"),
                "SCOPED_NEEDLE inside\n",
            )
            .unwrap();
            let marker_file = outside.join("outside-marker.txt");
            fs::write(
                &marker_file,
                format!("{EXTERNAL_SECRET_MARKER} SCOPED_NEEDLE\n"),
            )
            .unwrap();
            fs::write(
                outside.join(format!("{EXTERNAL_SECRET_MARKER}.txt")),
                "external filename sentinel\n",
            )
            .unwrap();
            let canonical_workspace = fs::canonicalize(&workspace).unwrap();
            Self {
                _temp: temp,
                workspace,
                canonical_workspace,
                outside,
                marker_file,
            }
        }

        fn tool_context(&self, max_output_bytes: usize) -> ToolContext {
            ctx(&self.workspace, max_output_bytes)
        }
    }

    #[cfg(unix)]
    fn create_file_symlink(target: &Path, link: &Path) -> io::Result<()> {
        std::os::unix::fs::symlink(target, link)
    }

    #[cfg(windows)]
    fn create_file_symlink(target: &Path, link: &Path) -> io::Result<()> {
        std::os::windows::fs::symlink_file(target, link)
    }

    #[cfg(unix)]
    fn create_directory_symlink(target: &Path, link: &Path) -> io::Result<()> {
        std::os::unix::fs::symlink(target, link)
    }

    #[cfg(windows)]
    fn create_directory_symlink(target: &Path, link: &Path) -> io::Result<()> {
        std::os::windows::fs::symlink_dir(target, link)
    }

    fn skip_link_test(kind: &str, err: &io::Error) {
        eprintln!(
            "SKIP {kind}: OS does not permit creating the required link; raw_os_error={:?}; error={err}",
            err.raw_os_error()
        );
    }

    #[tokio::test]
    async fn scoped_read_file_allows_relative_and_canonical_absolute_paths_inside_root() {
        let fixture = ContainmentFixture::new();
        let tool_context = fixture.tool_context(4096);
        let tool = ReadFileTool;
        let canonical_file =
            fs::canonicalize(fixture.workspace.join("inside").join("visible.txt")).unwrap();

        let relative = execute_scoped(
            &tool,
            json!({ "path": "inside/visible.txt" }),
            &tool_context,
            Some(&fixture.canonical_workspace),
        )
        .await;
        let absolute = execute_scoped(
            &tool,
            json!({ "path": path_arg(&canonical_file) }),
            &tool_context,
            Some(&fixture.canonical_workspace),
        )
        .await;

        assert_eq!(relative, absolute);
        assert_eq!(relative.content, "SCOPED_NEEDLE inside\n");
        assert!(!relative.is_error);
        assert!(!relative.truncated);
        assert_eq!(relative.exit, None);
    }

    #[tokio::test]
    async fn scoped_directory_tools_allow_relative_and_canonical_root_directory() {
        let fixture = ContainmentFixture::new();
        let tool_context = fixture.tool_context(4096);
        let list_dir = ListDirTool;
        let glob = GlobTool;
        let grep = GrepTool;
        let tools: [&dyn Tool; 3] = [&list_dir, &glob, &grep];

        for tool in tools {
            let relative = execute_scoped(
                tool,
                directory_args(tool, "."),
                &tool_context,
                Some(&fixture.canonical_workspace),
            )
            .await;
            let absolute = execute_scoped(
                tool,
                directory_args(tool, path_arg(&fixture.canonical_workspace)),
                &tool_context,
                Some(&fixture.canonical_workspace),
            )
            .await;

            assert_eq!(
                relative,
                absolute,
                "{} changed results between relative and canonical root paths",
                tool.name()
            );
            assert!(
                !relative.is_error,
                "{} rejected the canonical read root itself: {relative:?}",
                tool.name()
            );
            assert_eq!(relative.exit, None);
            match tool.name() {
                "list_dir" => assert!(relative.content.contains("inside")),
                "glob" => assert!(relative.content.contains("inside/visible.txt")),
                "grep" => assert!(relative.content.contains("SCOPED_NEEDLE inside")),
                _ => unreachable!(),
            }
        }
    }

    #[tokio::test]
    async fn scoped_read_file_root_directory_keeps_existing_directory_read_error() {
        let fixture = ContainmentFixture::new();
        let tool_context = fixture.tool_context(4096);
        let outcome = execute_scoped(
            &ReadFileTool,
            json!({ "path": path_arg(&fixture.canonical_workspace) }),
            &tool_context,
            Some(&fixture.canonical_workspace),
        )
        .await;

        assert!(outcome.is_error, "reading a directory must remain an error");
        assert!(
            outcome.content.contains("failed to read"),
            "directory input passed containment but lost the existing read error: {outcome:?}"
        );
        assert!(
            !outcome.content.contains("read root"),
            "the root directory itself must not be classified as an escape: {outcome:?}"
        );
        assert!(!outcome.truncated);
        assert_eq!(outcome.exit, None);
    }

    #[tokio::test]
    async fn scoped_fs_missing_paths_return_stable_errors() {
        let fixture = ContainmentFixture::new();
        let tool_context = fixture.tool_context(4096);
        let read_file = ReadFileTool;
        let list_dir = ListDirTool;
        let glob = GlobTool;
        let grep = GrepTool;
        let cases = [
            (&read_file as &dyn Tool, json!({ "path": "missing.txt" })),
            (&list_dir as &dyn Tool, json!({ "path": "missing" })),
            (
                &glob as &dyn Tool,
                json!({ "path": "missing", "pattern": "*.txt" }),
            ),
            (
                &grep as &dyn Tool,
                json!({ "path": "missing", "pattern": "needle" }),
            ),
        ];

        for (tool, args) in cases {
            let first = execute_scoped(
                tool,
                args.clone(),
                &tool_context,
                Some(&fixture.canonical_workspace),
            )
            .await;
            let second = execute_scoped(
                tool,
                args,
                &tool_context,
                Some(&fixture.canonical_workspace),
            )
            .await;

            assert_eq!(first, second, "{} missing-path error drifted", tool.name());
            assert!(first.is_error, "{} accepted a missing path", tool.name());
            assert!(!first.truncated);
            assert_eq!(first.exit, None);
        }
    }

    #[tokio::test]
    async fn scoped_fs_rejects_absolute_and_parent_escapes_before_target_io() {
        let fixture = ContainmentFixture::new();
        let tool_context = fixture.tool_context(4096);
        let read_file = ReadFileTool;
        let list_dir = ListDirTool;
        let glob = GlobTool;
        let grep = GrepTool;
        let tools: [&dyn Tool; 4] = [&read_file, &list_dir, &glob, &grep];
        let mut failures = Vec::new();

        for tool in tools {
            let (absolute_input, parent_input, absolute_probe, parent_probe) =
                if tool.name() == "read_file" {
                    (
                        path_arg(&fixture.marker_file),
                        "../workspace-escape/outside-marker.txt".to_string(),
                        fixture.marker_file.clone(),
                        fixture
                            .workspace
                            .join("../workspace-escape/outside-marker.txt"),
                    )
                } else {
                    (
                        path_arg(&fixture.outside),
                        "../workspace-escape".to_string(),
                        fixture.outside.clone(),
                        fixture.workspace.join("../workspace-escape"),
                    )
                };

            for (kind, input, probe_path) in [
                ("absolute", absolute_input, absolute_probe),
                ("parent", parent_input, parent_probe),
            ] {
                let args = if tool.name() == "read_file" {
                    json!({ "path": input })
                } else {
                    directory_args(tool, input)
                };
                let probe = FsProbe::install(probe_path);
                let outcome = tokio::time::timeout(
                    Duration::from_secs(5),
                    execute_scoped(
                        tool,
                        args,
                        &tool_context,
                        Some(&fixture.canonical_workspace),
                    ),
                )
                .await
                .unwrap_or_else(|_| {
                    panic!(
                        "{} {kind} escape did not release the shared blocking permit",
                        tool.name()
                    )
                });
                let target_io_count = probe.target_io_count();

                if !outcome.is_error
                    || target_io_count != 0
                    || outcome.content.contains(EXTERNAL_SECRET_MARKER)
                {
                    failures.push(format!(
                        "{} {kind}: is_error={}, target_io_count={target_io_count}, content={:?}",
                        tool.name(),
                        outcome.is_error,
                        outcome.content
                    ));
                }
            }
        }

        assert!(
            failures.is_empty(),
            "scoped filesystem escapes reached target I/O:\n{}",
            failures.join("\n")
        );
    }

    #[tokio::test]
    async fn scoped_fs_canonicalize_and_target_io_share_one_blocking_worker() {
        let fixture = ContainmentFixture::new();
        let tool_context = fixture.tool_context(4096);
        let canonical_file =
            fs::canonicalize(fixture.workspace.join("inside").join("visible.txt")).unwrap();
        let read_file = ReadFileTool;
        let list_dir = ListDirTool;
        let glob = GlobTool;
        let grep = GrepTool;
        let cases = [
            (
                &read_file as &dyn Tool,
                json!({ "path": path_arg(&canonical_file) }),
                canonical_file,
            ),
            (
                &list_dir as &dyn Tool,
                directory_args(&list_dir, path_arg(&fixture.canonical_workspace)),
                fixture.canonical_workspace.clone(),
            ),
            (
                &glob as &dyn Tool,
                directory_args(&glob, path_arg(&fixture.canonical_workspace)),
                fixture.canonical_workspace.clone(),
            ),
            (
                &grep as &dyn Tool,
                directory_args(&grep, path_arg(&fixture.canonical_workspace)),
                fixture.canonical_workspace.clone(),
            ),
        ];
        let caller_thread = std::thread::current().id();
        let mut invocation_ids = std::collections::HashSet::new();

        for (tool, args, target) in cases {
            let probe = FsProbe::install(target);
            let outcome = execute_scoped(
                tool,
                args,
                &tool_context,
                Some(&fixture.canonical_workspace),
            )
            .await;
            let events = probe.events();

            assert!(!outcome.is_error, "{} failed: {outcome:?}", tool.name());
            assert_eq!(
                events.iter().map(|event| event.stage).collect::<Vec<_>>(),
                vec![FsProbeStage::Canonicalize, FsProbeStage::TargetIo],
                "{} must canonicalize before target I/O",
                tool.name()
            );
            assert_eq!(
                events[0].thread,
                events[1].thread,
                "{} left the shared blocking worker",
                tool.name()
            );
            assert_ne!(
                events[0].thread,
                caller_thread,
                "{} containment ran on the async caller thread",
                tool.name()
            );
            let invocation_id = events[0]
                .invocation_id
                .unwrap_or_else(|| panic!("{} canonicalize missed invocation id", tool.name()));
            assert_eq!(
                events[1].invocation_id,
                Some(invocation_id),
                "{} target I/O crossed invocation identity",
                tool.name()
            );
            assert!(
                invocation_ids.insert(invocation_id),
                "{} reused a scoped invocation id",
                tool.name()
            );
        }
    }

    #[tokio::test]
    async fn scoped_read_file_rejects_external_file_symlink_before_content_io() {
        let fixture = ContainmentFixture::new();
        let link = fixture.workspace.join("outside-file-link.txt");
        if let Err(err) = create_file_symlink(&fixture.marker_file, &link) {
            skip_link_test("file symlink containment", &err);
            return;
        }
        let probe = FsProbe::install(link.clone());

        let outcome = execute_scoped(
            &ReadFileTool,
            json!({ "path": "outside-file-link.txt" }),
            &fixture.tool_context(4096),
            Some(&fixture.canonical_workspace),
        )
        .await;

        assert!(
            outcome.is_error
                && probe.target_io_count() == 0
                && !outcome.content.contains(EXTERNAL_SECRET_MARKER),
            "external file symlink reached content I/O: count={}, outcome={outcome:?}",
            probe.target_io_count()
        );
    }

    #[tokio::test]
    async fn scoped_directory_tools_reject_external_directory_symlink_before_walk() {
        let fixture = ContainmentFixture::new();
        let link = fixture.workspace.join("outside-directory-link");
        if let Err(err) = create_directory_symlink(&fixture.outside, &link) {
            skip_link_test("directory symlink containment", &err);
            return;
        }
        let tool_context = fixture.tool_context(4096);
        let list_dir = ListDirTool;
        let glob = GlobTool;
        let grep = GrepTool;
        let tools: [&dyn Tool; 3] = [&list_dir, &glob, &grep];
        let mut failures = Vec::new();

        for tool in tools {
            let probe = FsProbe::install(link.clone());
            let outcome = execute_scoped(
                tool,
                directory_args(tool, "outside-directory-link"),
                &tool_context,
                Some(&fixture.canonical_workspace),
            )
            .await;
            let target_io_count = probe.target_io_count();
            if !outcome.is_error
                || target_io_count != 0
                || outcome.content.contains(EXTERNAL_SECRET_MARKER)
            {
                failures.push(format!(
                    "{}: is_error={}, target_io_count={target_io_count}, content={:?}",
                    tool.name(),
                    outcome.is_error,
                    outcome.content
                ));
            }
        }

        assert!(
            failures.is_empty(),
            "external directory symlink reached directory walk:\n{}",
            failures.join("\n")
        );
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn scoped_directory_tools_reject_external_junction_before_walk() {
        let fixture = ContainmentFixture::new();
        let junction = fixture.workspace.join("outside-junction");
        let output = match std::process::Command::new("cmd.exe")
            .arg("/D")
            .arg("/C")
            .arg("mklink")
            .arg("/J")
            .arg(&junction)
            .arg(&fixture.outside)
            .output()
        {
            Ok(output) => output,
            Err(err) => {
                skip_link_test("Windows junction containment", &err);
                return;
            }
        };
        if !output.status.success() {
            eprintln!(
                "SKIP Windows junction containment: mklink /J failed; status={}; stdout={}; stderr={}",
                output.status,
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            return;
        }

        let tool_context = fixture.tool_context(4096);
        let list_dir = ListDirTool;
        let glob = GlobTool;
        let grep = GrepTool;
        let tools: [&dyn Tool; 3] = [&list_dir, &glob, &grep];
        let mut failures = Vec::new();

        let linked_file = junction.join("outside-marker.txt");
        let file_probe = FsProbe::install(linked_file);
        let file_outcome = execute_scoped(
            &ReadFileTool,
            json!({ "path": "outside-junction/outside-marker.txt" }),
            &tool_context,
            Some(&fixture.canonical_workspace),
        )
        .await;
        if !file_outcome.is_error
            || file_probe.target_io_count() != 0
            || file_outcome.content.contains(EXTERNAL_SECRET_MARKER)
        {
            failures.push(format!(
                "read_file: is_error={}, target_io_count={}, content={:?}",
                file_outcome.is_error,
                file_probe.target_io_count(),
                file_outcome.content
            ));
        }
        drop(file_probe);

        for tool in tools {
            let probe = FsProbe::install(junction.clone());
            let outcome = execute_scoped(
                tool,
                directory_args(tool, "outside-junction"),
                &tool_context,
                Some(&fixture.canonical_workspace),
            )
            .await;
            let target_io_count = probe.target_io_count();
            if !outcome.is_error
                || target_io_count != 0
                || outcome.content.contains(EXTERNAL_SECRET_MARKER)
            {
                failures.push(format!(
                    "{}: is_error={}, target_io_count={target_io_count}, content={:?}",
                    tool.name(),
                    outcome.is_error,
                    outcome.content
                ));
            }
        }

        for tool in [&glob as &dyn Tool, &grep as &dyn Tool] {
            let outcome = execute_scoped(
                tool,
                directory_args(tool, "."),
                &tool_context,
                Some(&fixture.canonical_workspace),
            )
            .await;
            if outcome.is_error || outcome.content.contains(EXTERNAL_SECRET_MARKER) {
                failures.push(format!(
                    "{} nested junction: is_error={}, content={:?}",
                    tool.name(),
                    outcome.is_error,
                    outcome.content
                ));
            }
        }

        assert!(
            failures.is_empty(),
            "external Windows junction reached file content or directory walk:\n{}",
            failures.join("\n")
        );
    }

    #[tokio::test]
    async fn scoped_glob_and_grep_do_not_follow_external_links_below_allowed_root() {
        let fixture = ContainmentFixture::new();
        let link = fixture.workspace.join("nested-external-link");
        if let Err(err) = create_directory_symlink(&fixture.outside, &link) {
            skip_link_test("nested directory symlink traversal", &err);
            return;
        }
        let tool_context = fixture.tool_context(4096);

        for tool in [&GlobTool as &dyn Tool, &GrepTool as &dyn Tool] {
            let outcome = execute_scoped(
                tool,
                directory_args(tool, "."),
                &tool_context,
                Some(&fixture.canonical_workspace),
            )
            .await;
            assert!(!outcome.is_error, "{} failed: {outcome:?}", tool.name());
            assert!(
                !outcome.content.contains(EXTERNAL_SECRET_MARKER),
                "{} followed an external link below the allowed root: {outcome:?}",
                tool.name()
            );
        }
    }

    #[tokio::test]
    async fn scoped_walkers_do_not_read_parent_ignore_rules_but_root_none_keeps_baseline() {
        let list_dir = ListDirTool;
        let glob = GlobTool;
        let grep = GrepTool;
        let tools: [&dyn Tool; 3] = [&list_dir, &glob, &grep];

        for ignore_name in [".ignore", ".gitignore"] {
            let temp = tempfile::tempdir().unwrap();
            let workspace = temp.path().join("workspace");
            fs::create_dir(&workspace).unwrap();
            fs::write(temp.path().join(ignore_name), "visible.txt\n").unwrap();
            fs::write(
                workspace.join("visible.txt"),
                "SCOPED_NEEDLE parent ignore must not cross read root\n",
            )
            .unwrap();
            let canonical_workspace = fs::canonicalize(&workspace).unwrap();
            let tool_context = ctx(&workspace, 4096);

            for tool in tools {
                let args = directory_args(tool, path_arg(&workspace));
                let root_none =
                    assert_root_none_matches_direct(tool, args.clone(), &tool_context).await;
                assert!(
                    !root_none.content.contains("visible.txt")
                        && !root_none.content.contains("SCOPED_NEEDLE"),
                    "{} root baseline did not apply parent {ignore_name}: {root_none:?}",
                    tool.name()
                );

                let scoped =
                    execute_scoped(tool, args, &tool_context, Some(&canonical_workspace)).await;
                assert!(!scoped.is_error, "{} failed: {scoped:?}", tool.name());
                assert!(
                    scoped.content.contains("visible.txt")
                        || scoped.content.contains("SCOPED_NEEDLE"),
                    "{} read parent {ignore_name} above canonical root: {scoped:?}",
                    tool.name()
                );
            }
        }
    }

    #[tokio::test]
    async fn scoped_walkers_reject_external_ignore_file_symlinks_before_actual_walk() {
        let list_dir = ListDirTool;
        let glob = GlobTool;
        let grep = GrepTool;
        let tools: [&dyn Tool; 3] = [&list_dir, &glob, &grep];

        for ignore_path in [PathBuf::from(".ignore"), PathBuf::from("nested/.gitignore")] {
            let fixture = ContainmentFixture::new();
            if let Some(parent) = ignore_path
                .parent()
                .filter(|parent| !parent.as_os_str().is_empty())
            {
                fs::create_dir_all(fixture.workspace.join(parent)).unwrap();
            }
            let ignore_name = ignore_path.file_name().unwrap().to_string_lossy();
            let external_rules = fixture
                .outside
                .join(format!("{EXTERNAL_SECRET_MARKER}-{ignore_name}-rules"));
            fs::write(
                &external_rules,
                format!("[{EXTERNAL_SECRET_MARKER}\nvisible.txt\n"),
            )
            .unwrap();
            let link = fixture.workspace.join(&ignore_path);
            if let Err(err) = create_file_symlink(&external_rules, &link) {
                skip_link_test("special ignore-file symlink containment", &err);
                return;
            }
            let tool_context = fixture.tool_context(4096);

            for tool in tools {
                let probe = FsProbe::install(fixture.canonical_workspace.clone());
                let outcome = execute_scoped(
                    tool,
                    directory_args(tool, "."),
                    &tool_context,
                    Some(&fixture.canonical_workspace),
                )
                .await;

                assert!(
                    outcome.is_error
                        && outcome.content == "path escapes read root"
                        && probe.target_io_count() == 0
                        && !outcome.content.contains(EXTERNAL_SECRET_MARKER),
                    "{} followed external {ignore_name} before containment: count={}, outcome={outcome:?}",
                    tool.name(),
                    probe.target_io_count()
                );
                drop(probe);
            }
        }
    }

    #[tokio::test]
    async fn scoped_walkers_preserve_parent_rules_and_hidden_behavior_for_descendant_target() {
        let temp = tempfile::tempdir().unwrap();
        let workspace = temp.path().join("workspace");
        let target = workspace.join(".hidden-target");
        fs::create_dir_all(&target).unwrap();
        fs::write(workspace.join(".ignore"), "parent-hidden.txt\n").unwrap();
        fs::write(
            target.join("parent-hidden.txt"),
            "SCOPED_NEEDLE parent hidden\n",
        )
        .unwrap();
        fs::write(target.join(".dot-hidden.txt"), "SCOPED_NEEDLE dot hidden\n").unwrap();
        fs::write(target.join("visible.txt"), "SCOPED_NEEDLE visible\n").unwrap();
        let canonical_workspace = fs::canonicalize(&workspace).unwrap();
        let tool_context = ctx(&workspace, 4096);
        let list_dir = ListDirTool;
        let glob = GlobTool;
        let grep = GrepTool;
        let tools: [&dyn Tool; 3] = [&list_dir, &glob, &grep];

        for tool in tools {
            let outcome = execute_scoped(
                tool,
                directory_args(tool, path_arg(&target)),
                &tool_context,
                Some(&canonical_workspace),
            )
            .await;
            assert!(!outcome.is_error, "{} failed: {outcome:?}", tool.name());
            assert!(
                !outcome.content.contains("parent-hidden")
                    && !outcome.content.contains("dot-hidden"),
                "{} changed parent ignore or hidden-entry semantics: {outcome:?}",
                tool.name()
            );
            assert!(
                outcome.content.contains("visible.txt")
                    || outcome.content.contains("SCOPED_NEEDLE visible"),
                "{} failed to traverse a hidden target root: {outcome:?}",
                tool.name()
            );
        }
    }

    #[tokio::test]
    async fn scoped_walkers_preserve_explicit_ignored_target_baseline() {
        let list_dir = ListDirTool;
        let glob = GlobTool;
        let grep = GrepTool;
        let tools: [&dyn Tool; 3] = [&list_dir, &glob, &grep];

        for (relative_target, parent_rule) in [
            ("ignored-target", "ignored-target/\n"),
            ("ignored-parent/target", "ignored-parent/\n"),
        ] {
            let temp = tempfile::tempdir().unwrap();
            let workspace = temp.path().join("workspace");
            let target = workspace.join(relative_target);
            fs::create_dir_all(&target).unwrap();
            fs::write(workspace.join(".ignore"), parent_rule).unwrap();
            fs::write(target.join(".gitignore"), "nested-hidden.txt\n").unwrap();
            fs::write(
                target.join("nested-hidden.txt"),
                "SCOPED_NEEDLE nested hidden\n",
            )
            .unwrap();
            fs::write(target.join("visible.txt"), "SCOPED_NEEDLE visible\n").unwrap();
            let canonical_workspace = fs::canonicalize(&workspace).unwrap();
            let tool_context = ctx(&workspace, 4096);

            for tool in tools {
                let args = directory_args(tool, path_arg(&target));
                let baseline =
                    assert_root_none_matches_direct(tool, args.clone(), &tool_context).await;
                assert!(
                    baseline.content.contains("visible.txt")
                        || baseline.content.contains("SCOPED_NEEDLE visible"),
                    "{} baseline did not enter explicit target {relative_target}: {baseline:?}",
                    tool.name()
                );
                assert!(
                    !baseline.content.contains("nested-hidden"),
                    "{} baseline lost target-local ignore semantics: {baseline:?}",
                    tool.name()
                );

                let scoped =
                    execute_scoped(tool, args, &tool_context, Some(&canonical_workspace)).await;
                assert_eq!(
                    scoped,
                    baseline,
                    "{} changed explicit ignored target behavior for {relative_target}",
                    tool.name()
                );
            }
        }
    }

    #[tokio::test]
    async fn scoped_walkers_preserve_ignore_precedence_and_whitelists() {
        let temp = tempfile::tempdir().unwrap();
        let workspace = temp.path().join("workspace");
        let target = workspace.join("nested");
        fs::create_dir_all(&target).unwrap();
        fs::write(workspace.join(".ignore"), "*.txt\n!visible.txt\n").unwrap();
        fs::write(workspace.join(".gitignore"), "visible.txt\n").unwrap();
        fs::write(
            target.join(".ignore"),
            "!nested-visible.txt\nnested-ignore-hidden.txt\n",
        )
        .unwrap();
        fs::write(
            target.join(".gitignore"),
            "nested-visible.txt\nnested-git-hidden.txt\n",
        )
        .unwrap();
        for (name, content) in [
            ("visible.txt", "SCOPED_NEEDLE visible\n"),
            ("nested-visible.txt", "SCOPED_NEEDLE nested visible\n"),
            ("nested-ignore-hidden.txt", "SCOPED_NEEDLE ignore hidden\n"),
            ("nested-git-hidden.txt", "SCOPED_NEEDLE git hidden\n"),
        ] {
            fs::write(target.join(name), content).unwrap();
        }
        let canonical_workspace = fs::canonicalize(&workspace).unwrap();
        let tool_context = ctx(&workspace, 4096);
        let list_dir = ListDirTool;
        let glob = GlobTool;
        let grep = GrepTool;
        let tools: [&dyn Tool; 3] = [&list_dir, &glob, &grep];

        for tool in tools {
            let args = directory_args(tool, path_arg(&target));
            let baseline = assert_root_none_matches_direct(tool, args.clone(), &tool_context).await;
            let scoped =
                execute_scoped(tool, args, &tool_context, Some(&canonical_workspace)).await;
            assert_eq!(
                scoped,
                baseline,
                "{} changed in-root ignore precedence or whitelist behavior",
                tool.name()
            );
            assert!(
                scoped.content.contains("visible")
                    && !scoped.content.contains("ignore-hidden")
                    && !scoped.content.contains("git-hidden"),
                "{} produced an unexpected precedence result: {scoped:?}",
                tool.name()
            );
        }
    }

    #[tokio::test]
    async fn scoped_walkers_do_not_probe_ignore_files_below_pruned_subtrees() {
        let temp = tempfile::tempdir().unwrap();
        let workspace = temp.path().join("workspace");
        let deep = workspace.join("ignored").join("deep");
        fs::create_dir_all(&deep).unwrap();
        fs::write(workspace.join(".ignore"), "ignored/\n").unwrap();
        let unreachable_ignore = deep.join(".gitignore");
        fs::write(&unreachable_ignore, "outside-marker\n").unwrap();
        let canonical_workspace = fs::canonicalize(&workspace).unwrap();
        let tool_context = ctx(&workspace, 4096);
        let probe = FsProbe::install(unreachable_ignore);
        let list_dir = ListDirTool;
        let glob = GlobTool;
        let grep = GrepTool;
        let tools: [&dyn Tool; 3] = [&list_dir, &glob, &grep];

        for tool in tools {
            let outcome = execute_scoped(
                tool,
                directory_args(tool, "."),
                &tool_context,
                Some(&canonical_workspace),
            )
            .await;
            assert!(!outcome.is_error, "{} failed: {outcome:?}", tool.name());
        }
        assert_eq!(
            probe.ignore_control_count(),
            0,
            "scoped preflight probed a control file below an ignored directory"
        );
    }

    #[tokio::test]
    async fn scoped_walkers_do_not_reject_external_ignore_links_below_pruned_subtrees() {
        let fixture = ContainmentFixture::new();
        let deep = fixture.workspace.join("ignored").join("deep");
        fs::create_dir_all(&deep).unwrap();
        fs::write(fixture.workspace.join(".ignore"), "ignored/\n").unwrap();
        let external_rules = fixture.outside.join("pruned-external-rules");
        fs::write(
            &external_rules,
            format!("[{EXTERNAL_SECRET_MARKER}\nvisible.txt\n"),
        )
        .unwrap();
        let link = deep.join(".gitignore");
        if let Err(err) = create_file_symlink(&external_rules, &link) {
            skip_link_test("pruned ignore-file symlink non-probing", &err);
            return;
        }
        let tool_context = fixture.tool_context(4096);
        let list_dir = ListDirTool;
        let glob = GlobTool;
        let grep = GrepTool;
        let tools: [&dyn Tool; 3] = [&list_dir, &glob, &grep];

        for tool in tools {
            let outcome = execute_scoped(
                tool,
                directory_args(tool, "."),
                &tool_context,
                Some(&fixture.canonical_workspace),
            )
            .await;
            assert!(
                !outcome.is_error && !outcome.content.contains(EXTERNAL_SECRET_MARKER),
                "{} probed an external control link below a pruned subtree: {outcome:?}",
                tool.name()
            );
        }
    }

    #[tokio::test]
    async fn scoped_walkers_preserve_internal_nested_ignore_semantics() {
        let temp = tempfile::tempdir().unwrap();
        let workspace = temp.path().join("workspace");
        let nested = workspace.join("nested");
        fs::create_dir_all(&nested).unwrap();
        fs::write(workspace.join(".ignore"), "root-hidden.txt\n").unwrap();
        fs::write(nested.join(".gitignore"), "nested-hidden.txt\n").unwrap();
        fs::write(
            workspace.join("root-hidden.txt"),
            "SCOPED_NEEDLE root hidden\n",
        )
        .unwrap();
        fs::write(
            nested.join("nested-hidden.txt"),
            "SCOPED_NEEDLE nested hidden\n",
        )
        .unwrap();
        fs::write(nested.join("visible.txt"), "SCOPED_NEEDLE visible\n").unwrap();
        let canonical_workspace = fs::canonicalize(&workspace).unwrap();
        let tool_context = ctx(&workspace, 4096);
        let list_dir = ListDirTool;
        let glob = GlobTool;
        let grep = GrepTool;
        let tools: [&dyn Tool; 3] = [&list_dir, &glob, &grep];

        for tool in tools {
            let outcome = execute_scoped(
                tool,
                directory_args(tool, "."),
                &tool_context,
                Some(&canonical_workspace),
            )
            .await;
            assert!(!outcome.is_error, "{} failed: {outcome:?}", tool.name());
            assert!(
                !outcome.content.contains("root-hidden.txt")
                    && !outcome.content.contains("nested-hidden.txt")
                    && !outcome.content.contains("root hidden")
                    && !outcome.content.contains("nested hidden"),
                "{} lost in-root ignore semantics: {outcome:?}",
                tool.name()
            );
            match tool.name() {
                "list_dir" => assert!(outcome.content.contains("nested")),
                "glob" => assert!(outcome.content.contains("nested/visible.txt")),
                "grep" => assert!(outcome.content.contains("SCOPED_NEEDLE visible")),
                _ => unreachable!(),
            }
        }
    }

    async fn assert_root_none_matches_direct(
        tool: &dyn Tool,
        args: serde_json::Value,
        tool_context: &ToolContext,
    ) -> ToolOutcome {
        let direct = tool.execute(args.clone(), tool_context).await;
        let scoped = execute_scoped(tool, args, tool_context, None).await;
        assert_eq!(
            scoped,
            direct,
            "{} changed legacy behavior when read_root=None",
            tool.name()
        );
        scoped
    }

    #[tokio::test]
    async fn scoped_root_none_preserves_external_paths_gitignore_truncation_and_errors() {
        let fixture = ContainmentFixture::new();
        fs::write(
            fixture.outside.join(".gitignore"),
            "ignored.txt\nignored-match.txt\n",
        )
        .unwrap();
        fs::write(fixture.outside.join("visible.txt"), "éééé SCOPED_NEEDLE\n").unwrap();
        fs::write(fixture.outside.join("ignored.txt"), "hidden").unwrap();
        fs::write(
            fixture.outside.join("ignored-match.txt"),
            "SCOPED_NEEDLE hidden",
        )
        .unwrap();
        let tool_context = fixture.tool_context(16);
        let read_file = ReadFileTool;
        let list_dir = ListDirTool;
        let glob = GlobTool;
        let grep = GrepTool;
        let tools: [&dyn Tool; 4] = [&read_file, &list_dir, &glob, &grep];

        let read_outcome = assert_root_none_matches_direct(
            &read_file,
            json!({ "path": path_arg(&fixture.outside.join("visible.txt")) }),
            &tool_context,
        )
        .await;
        assert!(!read_outcome.is_error);
        assert!(read_outcome.truncated);
        assert!(read_outcome.content.len() <= tool_context.max_output_bytes);

        let list_outcome = assert_root_none_matches_direct(
            &list_dir,
            json!({ "path": path_arg(&fixture.outside) }),
            &tool_context,
        )
        .await;
        assert!(!list_outcome.is_error);
        assert!(list_outcome.content.contains("visible.txt"));
        assert!(!list_outcome.content.contains("ignored.txt"));

        let glob_outcome = assert_root_none_matches_direct(
            &glob,
            json!({ "path": path_arg(&fixture.outside), "pattern": "*.txt" }),
            &tool_context,
        )
        .await;
        assert!(!glob_outcome.is_error);
        assert!(glob_outcome.content.contains("visible.txt"));
        assert!(!glob_outcome.content.contains("ignored-match.txt"));

        let grep_outcome = assert_root_none_matches_direct(
            &grep,
            json!({ "path": path_arg(&fixture.outside), "pattern": "SCOPED_NEEDLE" }),
            &tool_context,
        )
        .await;
        assert!(!grep_outcome.is_error);
        assert!(grep_outcome.truncated);
        assert!(!grep_outcome.content.contains("hidden"));

        for tool in tools {
            let missing = if tool.name() == "read_file" {
                json!({ "path": path_arg(&fixture.outside.join("missing.txt")) })
            } else {
                directory_args(tool, path_arg(&fixture.outside.join("missing")))
            };
            let missing_outcome =
                assert_root_none_matches_direct(tool, missing, &tool_context).await;
            assert!(missing_outcome.is_error);
            assert!(!missing_outcome.truncated);
            assert_eq!(missing_outcome.exit, None);
            assert_eq!(tool.permission_level(), PermissionLevel::ReadOnly);
            assert_eq!(tool.concurrency(), ToolConcurrency::ParallelSafe);
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
