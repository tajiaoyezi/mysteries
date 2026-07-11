use crate::permission::PermissionMode;
use crate::provider::ToolSchema;
use async_trait::async_trait;
use serde_json::Value;
use std::path::PathBuf;
use std::sync::{Arc, LazyLock};
use thiserror::Error;
use tokio::sync::Semaphore;

pub mod ask;
pub mod edit;
pub mod fs;
pub mod plan;
pub mod shell;
pub mod web;

#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn schema(&self) -> Value;
    fn permission_level(&self) -> PermissionLevel;

    /// 工具并发策略。默认 `Exclusive`（fail-safe 串行）；仅显式 opt-in 的工具可返回 `ParallelSafe`。
    fn concurrency(&self) -> ToolConcurrency {
        ToolConcurrency::Exclusive
    }

    fn network_permission_preview(&self, args: &Value) -> NetworkPermissionPreview {
        NetworkPermissionPreview {
            authorizable: false,
            full_args: args.clone(),
            canonical_initial_target: None,
            scope: None,
            denial_reason: Some("network tool does not provide a permission preview".to_string()),
        }
    }

    fn plan_only(&self) -> bool {
        false
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> ToolOutcome;
}

#[derive(Default)]
pub struct ToolRegistry {
    tools: Vec<Box<dyn Tool>>,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ToolRegistryError {
    #[error("duplicate tool registration: {0}")]
    Duplicate(String),
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, tool: Box<dyn Tool>) -> Result<(), ToolRegistryError> {
        let name = tool.name();
        if self
            .tools
            .iter()
            .any(|registered| registered.name() == name)
        {
            return Err(ToolRegistryError::Duplicate(name.to_string()));
        }

        self.tools.push(tool);
        Ok(())
    }

    pub fn get(&self, name: &str) -> Option<&dyn Tool> {
        self.tools
            .iter()
            .find(|tool| tool.name() == name)
            .map(|tool| tool.as_ref())
    }

    pub fn schemas(&self) -> Vec<ToolSchema> {
        self.tools
            .iter()
            .map(|tool| ToolSchema {
                name: tool.name().to_string(),
                description: tool.description().to_string(),
                parameters: tool.schema(),
            })
            .collect()
    }

    pub fn schemas_for(&self, mode: PermissionMode) -> Vec<ToolSchema> {
        self.tools
            .iter()
            .filter(|tool| match mode {
                PermissionMode::Plan => {
                    matches!(
                        tool.permission_level(),
                        PermissionLevel::ReadOnly | PermissionLevel::Network
                    ) || tool.plan_only()
                }
                _ => !tool.plan_only(),
            })
            .map(|tool| ToolSchema {
                name: tool.name().to_string(),
                description: tool.description().to_string(),
                parameters: tool.schema(),
            })
            .collect()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ToolOutcome {
    pub content: String,
    pub is_error: bool,
    pub truncated: bool,
    pub exit: Option<i32>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NetworkPermissionScope {
    pub max_redirects: u32,
    pub may_cross_origin: bool,
    pub ssrf_each_hop: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct NetworkPermissionPreview {
    pub authorizable: bool,
    pub full_args: Value,
    pub canonical_initial_target: Option<String>,
    pub scope: Option<NetworkPermissionScope>,
    pub denial_reason: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ToolContext {
    pub cwd: PathBuf,
    pub max_output_bytes: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PermissionLevel {
    ReadOnly,
    Network,
    Edit,
    Execute,
}

/// 工具并发策略；与 `PermissionLevel` 正交，不得从权限级别推断。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolConcurrency {
    /// 必须串行；与其他调用重叠可能破坏共享状态或产生顺序相关副作用。
    Exclusive,
    /// 显式声明：execute 可与其他同类调用重叠。
    ParallelSafe,
}

/// 进程级 blocking 文件工具并发上限（与 per-Agent `MAX_PARALLEL_TOOL_CALLS` 同值）。
pub const MAX_BLOCKING_TOOL_CALLS: usize = 4;

/// 可测试的 blocking offload 限流器；生产用进程共享实例，测试注入独立实例。
#[derive(Clone, Debug)]
pub struct BlockingToolLimiter {
    semaphore: Arc<Semaphore>,
}

impl BlockingToolLimiter {
    pub fn new(limit: usize) -> Self {
        Self {
            semaphore: Arc::new(Semaphore::new(limit)),
        }
    }
}

static PROCESS_BLOCKING_LIMITER: LazyLock<BlockingToolLimiter> =
    LazyLock::new(|| BlockingToolLimiter::new(MAX_BLOCKING_TOOL_CALLS));

/// 生产路径使用的进程共享 limiter。
pub fn process_blocking_limiter() -> BlockingToolLimiter {
    PROCESS_BLOCKING_LIMITER.clone()
}

/// 在 blocking worker 上执行同步工具主体。
///
/// 先异步 acquire owned permit，再将 permit 移入 `spawn_blocking` closure，
/// 直到同步工作真实结束后才释放。JoinError（含 worker panic）映射为
/// `ToolOutcome{is_error:true}`，不 panic。
pub async fn run_blocking_tool<F>(limiter: &BlockingToolLimiter, work: F) -> ToolOutcome
where
    F: FnOnce() -> ToolOutcome + Send + 'static,
{
    let permit = match limiter.semaphore.clone().acquire_owned().await {
        Ok(permit) => permit,
        Err(_) => {
            return ToolOutcome {
                content: "blocking worker failed: limiter closed".to_string(),
                is_error: true,
                truncated: false,
                exit: None,
            };
        }
    };

    match tokio::task::spawn_blocking(move || {
        // permit 持有到 work 返回；JoinHandle drop 后 closure 仍占容量。
        let _permit = permit;
        work()
    })
    .await
    {
        Ok(outcome) => outcome,
        Err(_) => ToolOutcome {
            content: "blocking worker failed".to_string(),
            is_error: true,
            truncated: false,
            exit: None,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::{
        PermissionLevel, Tool, ToolConcurrency, ToolContext, ToolOutcome, ToolRegistry,
        ToolRegistryError,
    };
    use crate::permission::PermissionMode;
    use crate::tool::ask::{Answer, AskUserTool, MockPrompter};
    use crate::tool::edit::{EditFileTool, WriteFileTool};
    use crate::tool::fs::{GlobTool, GrepTool, ListDirTool, ReadFileTool};
    use crate::tool::plan::{
        MockPlanApprover, MockPlanProgressReporter, PlanDecision, SubmitPlanTool, UpdatePlanTool,
    };
    use crate::tool::shell::RunShellTool;
    use crate::tool::web::{ReqwestFetcher, WebFetchTool, WebSearchTool};
    use async_trait::async_trait;
    use serde_json::{json, Value};
    use std::path::PathBuf;

    struct MockTool {
        name: &'static str,
        description: &'static str,
        permission_level: PermissionLevel,
        plan_only: bool,
    }

    #[async_trait]
    impl Tool for MockTool {
        fn name(&self) -> &str {
            self.name
        }

        fn description(&self) -> &str {
            self.description
        }

        fn schema(&self) -> Value {
            json!({
                "type": "object",
                "properties": {
                    "input": { "type": "string" }
                },
                "required": ["input"]
            })
        }

        fn permission_level(&self) -> PermissionLevel {
            self.permission_level.clone()
        }

        // 故意不 override concurrency()，以锁 trait default = Exclusive。

        fn plan_only(&self) -> bool {
            self.plan_only
        }

        async fn execute(&self, args: Value, _ctx: &ToolContext) -> ToolOutcome {
            ToolOutcome {
                content: format!("{}:{}", self.name, args["input"].as_str().unwrap()),
                is_error: false,
                truncated: false,
                exit: None,
            }
        }
    }

    /// 显式 override concurrency 的 mock，用于 registry 透传与分类查询。
    struct ClassifiedMockTool {
        name: &'static str,
        description: &'static str,
        permission_level: PermissionLevel,
        concurrency: ToolConcurrency,
    }

    #[async_trait]
    impl Tool for ClassifiedMockTool {
        fn name(&self) -> &str {
            self.name
        }

        fn description(&self) -> &str {
            self.description
        }

        fn schema(&self) -> Value {
            json!({
                "type": "object",
                "properties": {
                    "input": { "type": "string" }
                },
                "required": ["input"]
            })
        }

        fn permission_level(&self) -> PermissionLevel {
            self.permission_level.clone()
        }

        fn concurrency(&self) -> ToolConcurrency {
            self.concurrency
        }

        async fn execute(&self, args: Value, _ctx: &ToolContext) -> ToolOutcome {
            ToolOutcome {
                content: format!("{}:{}", self.name, args["input"].as_str().unwrap()),
                is_error: false,
                truncated: false,
                exit: None,
            }
        }
    }

    fn mock_tool(
        name: &'static str,
        description: &'static str,
        permission_level: PermissionLevel,
    ) -> MockTool {
        MockTool {
            name,
            description,
            permission_level,
            plan_only: false,
        }
    }

    fn plan_only_tool(name: &'static str) -> MockTool {
        MockTool {
            name,
            description: "Plan-only tool",
            permission_level: PermissionLevel::ReadOnly,
            plan_only: true,
        }
    }

    /// 完整 12 工具 fixture（含 plan / ask），不得用仅 9 个基础工具的 `default_registry()`。
    fn twelve_tool_registry() -> ToolRegistry {
        let mut registry = ToolRegistry::new();
        registry.register(Box::new(ListDirTool)).unwrap();
        registry.register(Box::new(ReadFileTool)).unwrap();
        registry.register(Box::new(GlobTool)).unwrap();
        registry.register(Box::new(GrepTool)).unwrap();
        registry.register(Box::new(WriteFileTool)).unwrap();
        registry.register(Box::new(EditFileTool)).unwrap();
        registry.register(Box::new(RunShellTool)).unwrap();
        registry
            .register(Box::new(WebFetchTool::new(Box::new(ReqwestFetcher::new()))))
            .unwrap();
        registry
            .register(Box::new(WebSearchTool::new(
                Box::new(ReqwestFetcher::new()),
            )))
            .unwrap();
        registry
            .register(Box::new(SubmitPlanTool::new(Box::new(
                MockPlanApprover::new(PlanDecision::Approve),
            ))))
            .unwrap();
        registry
            .register(Box::new(UpdatePlanTool::new(Box::new(
                MockPlanProgressReporter::new(),
            ))))
            .unwrap();
        registry
            .register(Box::new(AskUserTool::new(Box::new(MockPrompter::new(
                Answer {
                    selected: Vec::new(),
                    supplement: None,
                },
            )))))
            .unwrap();
        registry
    }

    fn ctx() -> ToolContext {
        ToolContext {
            cwd: PathBuf::from("."),
            max_output_bytes: 4096,
        }
    }

    #[tokio::test]
    async fn registry_registers_finds_and_executes_tools_by_name() {
        let mut registry = ToolRegistry::new();
        registry
            .register(Box::new(mock_tool(
                "read_mock",
                "Read mock data",
                PermissionLevel::ReadOnly,
            )))
            .unwrap();

        let tool = registry.get("read_mock").unwrap();
        let outcome = tool.execute(json!({ "input": "abc" }), &ctx()).await;

        assert_eq!(tool.permission_level(), PermissionLevel::ReadOnly);
        assert_eq!(
            outcome,
            ToolOutcome {
                content: "read_mock:abc".to_string(),
                is_error: false,
                truncated: false,
                exit: None,
            }
        );
        assert!(registry.get("missing").is_none());
    }

    #[test]
    fn registry_exposes_tool_schemas_for_model_requests() {
        let mut registry = ToolRegistry::new();
        registry
            .register(Box::new(mock_tool(
                "read_mock",
                "Read mock data",
                PermissionLevel::ReadOnly,
            )))
            .unwrap();
        registry
            .register(Box::new(mock_tool(
                "write_mock",
                "Write mock data",
                PermissionLevel::Edit,
            )))
            .unwrap();

        let schemas = registry.schemas();

        assert_eq!(schemas.len(), 2);
        assert_eq!(schemas[0].name, "read_mock");
        assert_eq!(schemas[0].description, "Read mock data");
        assert_eq!(
            schemas[0].parameters,
            json!({
                "type": "object",
                "properties": {
                    "input": { "type": "string" }
                },
                "required": ["input"]
            })
        );
        assert_eq!(schemas[1].name, "write_mock");
        assert_eq!(schemas[1].description, "Write mock data");
    }

    #[test]
    fn registry_rejects_duplicate_tool_name_without_overwriting_original() {
        let mut registry = ToolRegistry::new();

        let first = registry.register(Box::new(mock_tool(
            "same",
            "First tool",
            PermissionLevel::ReadOnly,
        )));
        let second = registry.register(Box::new(mock_tool(
            "same",
            "Second tool",
            PermissionLevel::Edit,
        )));

        assert_eq!(first, Ok(()));
        assert_eq!(
            second,
            Err(ToolRegistryError::Duplicate("same".to_string()))
        );
        assert_eq!(registry.get("same").unwrap().description(), "First tool");
        assert_eq!(registry.schemas().len(), 1);
    }

    #[test]
    fn registry_accepts_unique_tool_name() {
        let mut registry = ToolRegistry::new();

        let result = registry.register(Box::new(mock_tool(
            "unique",
            "Unique tool",
            PermissionLevel::ReadOnly,
        )));

        assert_eq!(result, Ok(()));
        assert!(registry.get("unique").is_some());
    }

    fn registry_with_mixed_tools() -> ToolRegistry {
        let mut registry = ToolRegistry::new();
        registry
            .register(Box::new(mock_tool(
                "read_tool",
                "Read tool",
                PermissionLevel::ReadOnly,
            )))
            .unwrap();
        registry
            .register(Box::new(mock_tool(
                "network_tool",
                "Network tool",
                PermissionLevel::Network,
            )))
            .unwrap();
        registry
            .register(Box::new(mock_tool(
                "edit_tool",
                "Edit tool",
                PermissionLevel::Edit,
            )))
            .unwrap();
        registry
            .register(Box::new(mock_tool(
                "exec_tool",
                "Execute tool",
                PermissionLevel::Execute,
            )))
            .unwrap();
        registry
            .register(Box::new(plan_only_tool("submit_plan")))
            .unwrap();
        registry
    }

    #[test]
    fn plan_only_defaults_false_for_unoverridden_tools() {
        let tool = mock_tool("plain", "Plain tool", PermissionLevel::ReadOnly);
        assert!(!tool.plan_only());
    }

    #[test]
    fn default_network_preview_is_reject_only_with_a_reason() {
        let args = json!({ "url": "https://example.com" });
        let tool = mock_tool("network_tool", "Network tool", PermissionLevel::Network);
        let preview = tool.network_permission_preview(&args);

        assert!(!preview.authorizable);
        assert!(preview
            .denial_reason
            .is_some_and(|reason| !reason.is_empty()));
        assert_eq!(preview.full_args, args);
    }

    #[test]
    fn schemas_for_plan_includes_readonly_network_and_plan_only_preserving_order() {
        let registry = registry_with_mixed_tools();
        let schemas = registry.schemas_for(PermissionMode::Plan);

        assert_eq!(
            schemas
                .iter()
                .map(|schema| schema.name.as_str())
                .collect::<Vec<_>>(),
            vec!["read_tool", "network_tool", "submit_plan"]
        );
    }

    #[test]
    fn schemas_for_non_plan_excludes_plan_only_preserving_order() {
        let registry = registry_with_mixed_tools();

        for mode in [
            PermissionMode::Normal,
            PermissionMode::AcceptEdits,
            PermissionMode::Yolo,
        ] {
            let schemas = registry.schemas_for(mode);
            assert_eq!(
                schemas
                    .iter()
                    .map(|schema| schema.name.as_str())
                    .collect::<Vec<_>>(),
                vec!["read_tool", "network_tool", "edit_tool", "exec_tool"],
                "mode={mode:?}"
            );
        }
    }

    #[test]
    fn schemas_unchanged_when_not_filtering_by_mode() {
        let registry = registry_with_mixed_tools();
        assert_eq!(registry.schemas().len(), 5);
        assert_eq!(
            registry
                .schemas()
                .iter()
                .map(|schema| schema.name.as_str())
                .collect::<Vec<_>>(),
            vec![
                "read_tool",
                "network_tool",
                "edit_tool",
                "exec_tool",
                "submit_plan"
            ]
        );
    }

    #[test]
    fn default_mock_concurrency_is_exclusive() {
        let tool = mock_tool("plain", "Plain tool", PermissionLevel::ReadOnly);
        assert_eq!(tool.concurrency(), ToolConcurrency::Exclusive);
        assert_eq!(tool.permission_level(), PermissionLevel::ReadOnly);
    }

    #[test]
    fn registry_exposes_explicit_mock_concurrency() {
        let mut registry = ToolRegistry::new();
        registry
            .register(Box::new(ClassifiedMockTool {
                name: "safe_mock",
                description: "Explicit ParallelSafe mock",
                permission_level: PermissionLevel::ReadOnly,
                concurrency: ToolConcurrency::ParallelSafe,
            }))
            .unwrap();

        let tool = registry.get("safe_mock").unwrap();
        assert_eq!(tool.concurrency(), ToolConcurrency::ParallelSafe);
        assert_eq!(tool.permission_level(), PermissionLevel::ReadOnly);
    }

    #[test]
    fn twelve_builtin_tools_concurrency_classification() {
        let registry = twelve_tool_registry();
        assert_eq!(registry.schemas().len(), 12);

        for name in ["list_dir", "read_file", "glob", "grep"] {
            let tool = registry.get(name).expect(name);
            assert_eq!(
                tool.concurrency(),
                ToolConcurrency::ParallelSafe,
                "{name} must be ParallelSafe"
            );
            assert_eq!(
                tool.permission_level(),
                PermissionLevel::ReadOnly,
                "{name} remains ReadOnly"
            );
        }

        for name in [
            "web_fetch",
            "web_search",
            "write_file",
            "edit_file",
            "run_shell",
            "submit_plan",
            "update_plan",
            "ask_user",
        ] {
            let tool = registry.get(name).expect(name);
            assert_eq!(
                tool.concurrency(),
                ToolConcurrency::Exclusive,
                "{name} must be Exclusive"
            );
        }
    }

    // --- BlockingToolLimiter / run_blocking_tool (§2.2 RED) ---

    use super::{run_blocking_tool, BlockingToolLimiter, MAX_BLOCKING_TOOL_CALLS};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use std::thread;
    use std::time::Duration;

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn run_blocking_tool_executes_on_different_thread() {
        let limiter = BlockingToolLimiter::new(MAX_BLOCKING_TOOL_CALLS);
        let caller = thread::current().id();
        let worker = run_blocking_tool(&limiter, move || {
            let id = thread::current().id();
            ToolOutcome {
                content: format!("{id:?}"),
                is_error: false,
                truncated: false,
                exit: None,
            }
        })
        .await;
        assert!(!worker.is_error);
        assert_ne!(
            worker.content,
            format!("{caller:?}"),
            "blocking work must leave the calling thread"
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn run_blocking_tool_does_not_block_current_thread_worker() {
        let limiter = BlockingToolLimiter::new(1);
        let (entered_tx, entered_rx) = tokio::sync::oneshot::channel::<()>();
        let (release_tx, release_rx) = std::sync::mpsc::channel::<()>();
        let (probe_tx, probe_rx) = tokio::sync::oneshot::channel::<()>();
        let order: Arc<Mutex<Vec<&'static str>>> = Arc::new(Mutex::new(Vec::new()));

        // 失败路径也必须 release，避免挂死 current_thread runtime。
        let release_slot = Arc::new(Mutex::new(Some(release_tx)));
        let fire_release: Arc<dyn Fn() + Send + Sync> = {
            let release_slot = release_slot.clone();
            Arc::new(move || {
                if let Ok(mut guard) = release_slot.lock() {
                    if let Some(tx) = guard.take() {
                        let _ = tx.send(());
                    }
                }
            })
        };
        let (watchdog_cancel_tx, watchdog_cancel_rx) = std::sync::mpsc::channel::<()>();
        let watchdog = {
            let fire_release = fire_release.clone();
            thread::spawn(move || {
                if watchdog_cancel_rx
                    .recv_timeout(Duration::from_secs(5))
                    .is_err()
                {
                    fire_release();
                }
            })
        };

        let order_for_blocking = order.clone();
        let blocking = tokio::spawn(async move {
            run_blocking_tool(&limiter, move || {
                let _ = entered_tx.send(());
                let _ = release_rx.recv();
                order_for_blocking.lock().unwrap().push("release");
                ToolOutcome {
                    content: "done".into(),
                    is_error: false,
                    truncated: false,
                    exit: None,
                }
            })
            .await
        });

        // 异步等待 entered（不得在 current_thread 上 std::recv 占死 worker）。
        tokio::time::timeout(Duration::from_secs(2), entered_rx)
            .await
            .expect("blocking closure should enter")
            .expect("entered oneshot");

        // 独立 async probe：若 blocking 占住 current_thread worker，probe 无法完成。
        tokio::spawn(async move {
            let _ = probe_tx.send(());
        });
        tokio::time::timeout(Duration::from_secs(2), probe_rx)
            .await
            .expect("probe future should resolve while blocking waits")
            .expect("probe oneshot");
        order.lock().unwrap().push("probe");

        // 现在才 release blocking work。
        fire_release();
        let _ = watchdog_cancel_tx.send(());
        let outcome = blocking.await.expect("blocking task join");
        assert!(!outcome.is_error);
        assert_eq!(outcome.content, "done");

        let recorded = order.lock().unwrap().clone();
        assert_eq!(
            recorded,
            vec!["probe", "release"],
            "async probe must complete before blocking release"
        );

        let _ = watchdog.join();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn run_blocking_tool_caps_max_active_at_four() {
        let limiter = BlockingToolLimiter::new(MAX_BLOCKING_TOOL_CALLS);
        let active = Arc::new(AtomicUsize::new(0));
        let max_active = Arc::new(AtomicUsize::new(0));
        let (entered_tx, entered_rx) = std::sync::mpsc::channel::<()>();
        let (release_tx, release_rx) = std::sync::mpsc::channel::<()>();
        let release_rx = Arc::new(Mutex::new(release_rx));
        let entered_tx = Arc::new(Mutex::new(entered_tx));

        let mut handles = Vec::new();
        for _ in 0..8 {
            let limiter = limiter.clone();
            let active = active.clone();
            let max_active = max_active.clone();
            let entered_tx = entered_tx.clone();
            let release_rx = release_rx.clone();
            handles.push(tokio::spawn(async move {
                run_blocking_tool(&limiter, move || {
                    let now = active.fetch_add(1, Ordering::SeqCst) + 1;
                    max_active.fetch_max(now, Ordering::SeqCst);
                    if let Ok(tx) = entered_tx.lock() {
                        let _ = tx.send(());
                    }
                    // 每个 closure 等同一个 release 广播：用 shared recv 排队。
                    let _ = release_rx.lock().unwrap().recv();
                    active.fetch_sub(1, Ordering::SeqCst);
                    ToolOutcome {
                        content: "ok".into(),
                        is_error: false,
                        truncated: false,
                        exit: None,
                    }
                })
                .await
            }));
        }

        // 应先有恰好 4 个 entered；若无 cap 会有 8 个。
        let mut entered = 0usize;
        while entered < MAX_BLOCKING_TOOL_CALLS {
            entered_rx
                .recv_timeout(Duration::from_secs(2))
                .expect("expected blocking entry under cap");
            entered += 1;
        }
        // 短暂窗口内不得出现第 5 个 entered。
        assert!(
            entered_rx.recv_timeout(Duration::from_millis(100)).is_err(),
            "fifth closure must wait for a free permit"
        );
        assert!(
            max_active.load(Ordering::SeqCst) <= MAX_BLOCKING_TOOL_CALLS,
            "max-active exceeded cap: {}",
            max_active.load(Ordering::SeqCst)
        );

        // 释放全部：向 8 个 waiter 各发一次（channel 无广播，循环 send）。
        for _ in 0..8 {
            let _ = release_tx.send(());
        }
        for handle in handles {
            let outcome = handle.await.expect("join task");
            assert!(!outcome.is_error);
        }
        assert_eq!(
            max_active.load(Ordering::SeqCst),
            MAX_BLOCKING_TOOL_CALLS,
            "max-active should reach exactly the cap"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn aborted_awaiting_batch_still_holds_permits_until_closures_finish() {
        // 审查问题 #5：必须真取消 awaiting future（abort + JoinError），不能 drop detach。
        let limiter = BlockingToolLimiter::new(MAX_BLOCKING_TOOL_CALLS);
        let (entered_tx, entered_rx) = std::sync::mpsc::channel::<()>();
        let (release_tx, release_rx) = std::sync::mpsc::channel::<()>();
        let (done_tx, done_rx) = std::sync::mpsc::channel::<()>();
        let release_rx = Arc::new(Mutex::new(release_rx));
        let entered_tx = Arc::new(Mutex::new(entered_tx));
        let done_tx = Arc::new(Mutex::new(done_tx));

        // OS watchdog：失败路径也 release；成功路径可提前结束（不固定睡 5s）。
        let release_for_wd = Arc::new(Mutex::new(Some(release_tx.clone())));
        let cancel_wd = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let wd = {
            let release_for_wd = release_for_wd.clone();
            let cancel_wd = cancel_wd.clone();
            thread::spawn(move || {
                for _ in 0..50 {
                    if cancel_wd.load(Ordering::SeqCst) {
                        return;
                    }
                    thread::sleep(Duration::from_millis(100));
                }
                if let Ok(mut g) = release_for_wd.lock() {
                    if let Some(tx) = g.take() {
                        for _ in 0..MAX_BLOCKING_TOOL_CALLS {
                            let _ = tx.send(());
                        }
                    }
                }
            })
        };

        let mut first_batch = Vec::new();
        for _ in 0..MAX_BLOCKING_TOOL_CALLS {
            let limiter = limiter.clone();
            let entered_tx = entered_tx.clone();
            let release_rx = release_rx.clone();
            let done_tx = done_tx.clone();
            first_batch.push(tokio::spawn(async move {
                run_blocking_tool(&limiter, move || {
                    if let Ok(tx) = entered_tx.lock() {
                        let _ = tx.send(());
                    }
                    let _ = release_rx.lock().unwrap().recv();
                    if let Ok(tx) = done_tx.lock() {
                        let _ = tx.send(());
                    }
                    ToolOutcome {
                        content: "first".into(),
                        is_error: false,
                        truncated: false,
                        exit: None,
                    }
                })
                .await
            }));
        }

        for _ in 0..MAX_BLOCKING_TOOL_CALLS {
            entered_rx
                .recv_timeout(Duration::from_secs(2))
                .expect("first batch should enter");
        }

        // 显式 abort 并等待 canceled JoinError（证明 awaiting future 真取消）。
        for handle in first_batch.drain(..) {
            handle.abort();
            let join_err = handle
                .await
                .expect_err("aborted task must not return Ok(outcome)");
            assert!(
                join_err.is_cancelled(),
                "aborted task must report cancelled JoinError, got {join_err:?}"
            );
        }
        assert_eq!(
            limiter.semaphore.available_permits(),
            0,
            "aborted awaiting futures must leave all permits inside running closures"
        );

        // 立即提交第二批；旧 closure 仍持 permit，不得 entered。
        let (second_entered_tx, second_entered_rx) = std::sync::mpsc::channel::<()>();
        let second_entered_tx = Arc::new(Mutex::new(second_entered_tx));
        let (queued_tx, mut queued_rx) = tokio::sync::mpsc::unbounded_channel::<()>();
        let mut second_batch = Vec::new();
        for _ in 0..MAX_BLOCKING_TOOL_CALLS {
            let limiter = limiter.clone();
            let second_entered_tx = second_entered_tx.clone();
            let queued_tx = queued_tx.clone();
            second_batch.push(tokio::spawn(async move {
                let _ = queued_tx.send(());
                run_blocking_tool(&limiter, move || {
                    if let Ok(tx) = second_entered_tx.lock() {
                        let _ = tx.send(());
                    }
                    ToolOutcome {
                        content: "second".into(),
                        is_error: false,
                        truncated: false,
                        exit: None,
                    }
                })
                .await
            }));
        }
        drop(queued_tx);
        for _ in 0..MAX_BLOCKING_TOOL_CALLS {
            queued_rx
                .recv()
                .await
                .expect("second batch task queued before checking permits");
        }
        assert_eq!(limiter.semaphore.available_permits(), 0);
        assert!(matches!(
            second_entered_rx.try_recv(),
            Err(std::sync::mpsc::TryRecvError::Empty)
        ));

        // 释放旧 closure。
        for _ in 0..MAX_BLOCKING_TOOL_CALLS {
            let _ = release_tx.send(());
        }
        if let Ok(mut g) = release_for_wd.lock() {
            let _ = g.take();
        }
        for _ in 0..MAX_BLOCKING_TOOL_CALLS {
            done_rx
                .recv_timeout(Duration::from_secs(2))
                .expect("first batch closures should finish after release");
        }

        for _ in 0..MAX_BLOCKING_TOOL_CALLS {
            second_entered_rx
                .recv_timeout(Duration::from_secs(2))
                .expect("second batch should enter after release");
        }
        for handle in second_batch {
            let outcome = handle.await.expect("second join");
            assert!(!outcome.is_error);
        }
        cancel_wd.store(true, Ordering::SeqCst);
        let _ = wd.join();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn run_blocking_tool_maps_worker_panic_to_tool_error() {
        let limiter = BlockingToolLimiter::new(1);
        let outcome = run_blocking_tool(&limiter, || {
            panic!("intentional blocking worker panic");
        })
        .await;
        assert!(
            outcome.is_error,
            "JoinError/panic must become is_error ToolOutcome, got: {outcome:?}"
        );
        assert!(
            !outcome.content.is_empty(),
            "error content must explain worker failure"
        );
    }
}
