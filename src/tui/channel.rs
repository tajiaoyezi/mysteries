use crate::agent::{AgentObserver, AgentStatus, RunIdentity};
use crate::config::append_allowed_command;
use crate::permission::{
    auto_allows, PermissionCheck, PermissionDecider, PermissionDecision, PermissionMode,
    PermissionReply, PolicyEngine,
};
use crate::provider::DeltaSink;
use crate::tool::ask::{Answer, Question, UserPrompter};
use crate::tool::plan::{
    Plan, PlanApprover, PlanDecision, PlanProgressReporter, PlanProgressUpdate,
};
use crate::tool::{NetworkPermissionPreview, PermissionLevel, ToolOutcome};
use async_trait::async_trait;
use serde_json::Value;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tokio::sync::{mpsc, oneshot};

#[derive(Debug)]
pub enum AgentEvent {
    TextDelta(String),
    ThinkingDelta(String),
    ToolCallStarted {
        id: String,
        name: String,
        args: Value,
        readonly: bool,
    },
    ToolCallFinished {
        id: String,
        outcome: ToolOutcome,
    },
    StatusChanged(AgentStatus),
    PermissionRequired(PermissionRequest),
    PlanApprovalRequired(PlanApprovalRequest),
    UserQuestionRequired(QuestionRequest),
    Interrupted,
    TurnComplete,
    /// 手动 /compact 完成(成功或失败均发):置回 Ready 并作为排队推进闸门事件。
    CompactDone,
    Notice(String),
    /// Agent 已成功切换 provider/model；UI 仅在此事件后提交状态栏。
    ProviderApplied {
        id: String,
        model: String,
    },
    Usage {
        input_tokens: u32,
        output_tokens: u32,
    },
    Error(String),
    PlanProgress(PlanProgressUpdate),
}

#[derive(Debug, PartialEq, Eq)]
pub enum UserInput {
    Prompt(String),
    SetModel(String),
    /// provider/model 切换；history 处理由 `kind` 明确区分交互切换与 session 恢复。
    SetProvider {
        id: String,
        model: String,
        kind: ProviderSwitchKind,
    },
    Compact,
    Interrupt,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProviderSwitchKind {
    /// 用户主动切换 model/provider：旧 thinking 与新模型不兼容，需清空。
    Interactive,
    /// session activation：历史属于被恢复的模型，必须逐字段保留 thinking。
    SessionRestore,
}

#[derive(Debug)]
pub struct PermissionRequest {
    pub tool_name: String,
    pub args: Value,
    pub permission_level: PermissionLevel,
    pub network_preview: Option<NetworkPermissionPreview>,
    pub allow_always_key: Option<String>,
    pub responder: oneshot::Sender<PermissionReply>,
}

#[derive(Debug)]
pub struct PlanApprovalRequest {
    pub plan: Plan,
    pub responder: oneshot::Sender<PlanDecision>,
}

#[derive(Debug)]
pub struct QuestionRequest {
    pub question: Question,
    pub responder: oneshot::Sender<Answer>,
}

pub struct ChannelSink {
    tx: mpsc::UnboundedSender<AgentEvent>,
}

impl ChannelSink {
    pub fn new(tx: mpsc::UnboundedSender<AgentEvent>) -> Self {
        Self { tx }
    }
}

impl DeltaSink for ChannelSink {
    fn on_text(&self, text: &str) {
        if text.is_empty() {
            return;
        }

        let _ = self.tx.send(AgentEvent::TextDelta(text.to_string()));
    }

    fn on_thinking(&self, text: &str) {
        if text.is_empty() {
            return;
        }

        let _ = self.tx.send(AgentEvent::ThinkingDelta(text.to_string()));
    }
}

pub struct ChannelObserver {
    tx: mpsc::UnboundedSender<AgentEvent>,
}

impl ChannelObserver {
    pub fn new(tx: mpsc::UnboundedSender<AgentEvent>) -> Self {
        Self { tx }
    }
}

impl AgentObserver for ChannelObserver {
    fn on_status(&self, status: AgentStatus) {
        let _ = self.tx.send(AgentEvent::StatusChanged(status));
    }

    fn on_scoped_status(&self, identity: &RunIdentity, status: AgentStatus) {
        if identity.parent_run_id().is_none() {
            self.on_status(status);
        }
    }

    fn on_tool_call_started(&self, id: &str, name: &str, args: &Value, readonly: bool) {
        let _ = self.tx.send(AgentEvent::ToolCallStarted {
            id: id.to_string(),
            name: name.to_string(),
            args: args.clone(),
            readonly,
        });
    }

    fn on_scoped_tool_call_started(
        &self,
        identity: &RunIdentity,
        id: &str,
        name: &str,
        args: &Value,
        readonly: bool,
    ) {
        if identity.parent_run_id().is_none() {
            self.on_tool_call_started(id, name, args, readonly);
        }
    }

    fn on_tool_call_finished(&self, id: &str, outcome: &ToolOutcome) {
        let _ = self.tx.send(AgentEvent::ToolCallFinished {
            id: id.to_string(),
            outcome: outcome.clone(),
        });
    }

    fn on_scoped_tool_call_finished(
        &self,
        identity: &RunIdentity,
        id: &str,
        outcome: &ToolOutcome,
    ) {
        if identity.parent_run_id().is_none() {
            self.on_tool_call_finished(id, outcome);
        }
    }

    fn on_usage(&self, usage: &crate::provider::Usage) {
        let _ = self.tx.send(AgentEvent::Usage {
            input_tokens: usage.input_tokens,
            output_tokens: usage.output_tokens,
        });
    }

    fn on_scoped_usage(&self, _identity: &RunIdentity, usage: &crate::provider::Usage) {
        self.on_usage(usage);
    }
}

pub struct ChannelDecider {
    tx: mpsc::UnboundedSender<AgentEvent>,
    mode: Arc<Mutex<PermissionMode>>,
    policy: Mutex<PolicyEngine>,
    user_config_path: PathBuf,
}

impl ChannelDecider {
    pub fn new(
        tx: mpsc::UnboundedSender<AgentEvent>,
        mode: Arc<Mutex<PermissionMode>>,
        policy: PolicyEngine,
        user_config_path: PathBuf,
    ) -> Self {
        Self {
            tx,
            mode,
            policy: Mutex::new(policy),
            user_config_path,
        }
    }
}

#[async_trait]
impl PermissionDecider for ChannelDecider {
    async fn decide(&self, check: PermissionCheck<'_>) -> PermissionDecision {
        let call = check.call;
        let tool = check.tool;
        let permission_level = tool.permission_level();
        let network_preview = check.network_preview.cloned();
        let reject_only_network = permission_level == PermissionLevel::Network
            && !network_preview_is_authorizable(network_preview.as_ref());

        if reject_only_network {
            let (tx, rx) = oneshot::channel();
            let request = PermissionRequest {
                tool_name: tool.name().to_string(),
                args: call.arguments.clone(),
                permission_level,
                network_preview,
                allow_always_key: None,
                responder: tx,
            };

            if self
                .tx
                .send(AgentEvent::PermissionRequired(request))
                .is_err()
            {
                return PermissionDecision::Deny;
            }

            return match rx.await.unwrap_or(PermissionReply::Deny) {
                PermissionReply::Deny
                | PermissionReply::AllowOnce
                | PermissionReply::AllowAlways => PermissionDecision::Deny,
            };
        }

        {
            let policy = self.policy.lock().expect("policy mutex poisoned");
            if policy.is_allowed(call, tool) {
                return PermissionDecision::Allow;
            }
        }

        let allow_always_key = PolicyEngine::permission_key(call, tool);
        let mode = *self.mode.lock().expect("permission mode mutex poisoned");
        if auto_allows(mode, permission_level.clone()) {
            return PermissionDecision::Allow;
        }

        let (tx, rx) = oneshot::channel();
        let request = PermissionRequest {
            tool_name: tool.name().to_string(),
            args: call.arguments.clone(),
            permission_level,
            network_preview,
            allow_always_key: allow_always_key.clone(),
            responder: tx,
        };

        if self
            .tx
            .send(AgentEvent::PermissionRequired(request))
            .is_err()
        {
            return PermissionDecision::Deny;
        }

        match rx.await.unwrap_or(PermissionReply::Deny) {
            PermissionReply::AllowOnce => PermissionDecision::Allow,
            PermissionReply::AllowAlways => {
                if let Some(key) = allow_always_key {
                    self.policy
                        .lock()
                        .expect("policy mutex poisoned")
                        .remember(key.clone());
                    if let Err(err) = append_allowed_command(&self.user_config_path, &key) {
                        let _ = self
                            .tx
                            .send(AgentEvent::Notice(format!("命令白名单持久化失败:{err}")));
                    }
                }
                PermissionDecision::Allow
            }
            PermissionReply::Deny => PermissionDecision::Deny,
        }
    }
}

fn network_preview_is_authorizable(preview: Option<&NetworkPermissionPreview>) -> bool {
    preview.is_some_and(|preview| {
        preview.authorizable
            && preview.canonical_initial_target.is_some()
            && preview.scope.is_some()
            && preview.denial_reason.is_none()
    })
}

pub struct ChannelPlanApprover {
    tx: mpsc::UnboundedSender<AgentEvent>,
    mode: Arc<Mutex<PermissionMode>>,
}

impl ChannelPlanApprover {
    pub fn new(tx: mpsc::UnboundedSender<AgentEvent>, mode: Arc<Mutex<PermissionMode>>) -> Self {
        Self { tx, mode }
    }
}

#[async_trait]
impl PlanApprover for ChannelPlanApprover {
    async fn approve(&self, plan: &Plan) -> PlanDecision {
        let (responder, rx) = oneshot::channel();
        let request = PlanApprovalRequest {
            plan: plan.clone(),
            responder,
        };

        if self
            .tx
            .send(AgentEvent::PlanApprovalRequired(request))
            .is_err()
        {
            return PlanDecision::Reject("UI unavailable".to_string());
        }

        let decision = rx
            .await
            .unwrap_or(PlanDecision::Reject("UI disconnected".to_string()));

        if matches!(decision, PlanDecision::Approve) {
            *self.mode.lock().expect("permission mode mutex poisoned") =
                PermissionMode::AcceptEdits;
        }

        decision
    }
}

pub struct ChannelPrompter {
    tx: mpsc::UnboundedSender<AgentEvent>,
}

impl ChannelPrompter {
    pub fn new(tx: mpsc::UnboundedSender<AgentEvent>) -> Self {
        Self { tx }
    }
}

#[async_trait]
impl UserPrompter for ChannelPrompter {
    async fn prompt(&self, question: &Question) -> Answer {
        let (responder, rx) = oneshot::channel();
        let request = QuestionRequest {
            question: question.clone(),
            responder,
        };

        if self
            .tx
            .send(AgentEvent::UserQuestionRequired(request))
            .is_err()
        {
            return Answer {
                selected: Vec::new(),
                supplement: None,
            };
        }

        rx.await.unwrap_or(Answer {
            selected: Vec::new(),
            supplement: None,
        })
    }
}

pub struct ChannelProgressReporter {
    tx: mpsc::UnboundedSender<AgentEvent>,
}

impl ChannelProgressReporter {
    pub fn new(tx: mpsc::UnboundedSender<AgentEvent>) -> Self {
        Self { tx }
    }
}

impl PlanProgressReporter for ChannelProgressReporter {
    fn report(&self, update: PlanProgressUpdate) {
        let _ = self.tx.send(AgentEvent::PlanProgress(update));
    }
}

#[cfg(test)]
mod tests {
    use super::{
        AgentEvent, ChannelDecider, ChannelObserver, ChannelPlanApprover, ChannelProgressReporter,
        ChannelPrompter, ChannelSink,
    };
    use crate::agent::{
        AgentExecutionScope, AgentObserver, AgentStatus, ExecutionBudget, ExecutionCapabilities,
        RunIdentity,
    };
    use crate::config::read_raw_config;
    use crate::permission::{
        PermissionCheck, PermissionDecider, PermissionDecision, PermissionMode, PermissionReply,
        PolicyEngine,
    };
    use crate::provider::{DeltaSink, ToolCall, Usage};
    use crate::tool::ask::{Question, UserPrompter};
    use crate::tool::plan::{
        Plan, PlanApprover, PlanDecision, PlanProgressReporter, PlanProgressUpdate, StepStatus,
    };
    use crate::tool::{
        NetworkPermissionPreview, NetworkPermissionScope, PermissionLevel, Tool, ToolContext,
        ToolOutcome,
    };
    use async_trait::async_trait;
    use serde_json::{json, Value};
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};
    use tokio::sync::mpsc;
    use tokio::time::{sleep, timeout, Duration};

    fn permission_check<'a>(call: &'a ToolCall, tool: &'a dyn Tool) -> PermissionCheck<'a> {
        PermissionCheck {
            call,
            tool,
            network_preview: None,
        }
    }

    fn network_permission_check<'a>(
        call: &'a ToolCall,
        tool: &'a dyn Tool,
        preview: &'a NetworkPermissionPreview,
    ) -> PermissionCheck<'a> {
        PermissionCheck {
            call,
            tool,
            network_preview: Some(preview),
        }
    }

    fn observer_identities() -> (RunIdentity, RunIdentity) {
        let capabilities =
            ExecutionCapabilities::try_new(["read_file"], [PermissionLevel::ReadOnly]).unwrap();
        let root =
            AgentExecutionScope::root(ExecutionBudget::new(8, None, 1), capabilities.clone());
        let child = root
            .derive_child(ExecutionBudget::new(8, None, 0), capabilities)
            .unwrap();
        (root.identity(), child.identity())
    }

    #[test]
    fn channel_sink_sends_text_delta_on_text() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let sink = ChannelSink::new(tx);

        sink.on_text("hello");

        match rx.try_recv().unwrap() {
            AgentEvent::TextDelta(text) => assert_eq!(text, "hello"),
            other => panic!("expected TextDelta, got {other:?}"),
        }
    }

    #[test]
    fn channel_observer_sends_status_changed() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let observer = ChannelObserver::new(tx);

        observer.on_status(AgentStatus::CallingModel);

        match rx.try_recv().unwrap() {
            AgentEvent::StatusChanged(status) => assert_eq!(status, AgentStatus::CallingModel),
            other => panic!("expected StatusChanged, got {other:?}"),
        }
    }

    #[test]
    fn channel_observer_sends_tool_call_started() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let observer = ChannelObserver::new(tx);

        observer.on_tool_call_started("call-1", "read_file", &json!({ "path": "note.txt" }), true);

        match rx.try_recv().unwrap() {
            AgentEvent::ToolCallStarted {
                id,
                name,
                args,
                readonly,
            } => {
                assert_eq!(id, "call-1");
                assert_eq!(name, "read_file");
                assert_eq!(args, json!({ "path": "note.txt" }));
                assert!(readonly);
            }
            other => panic!("expected ToolCallStarted, got {other:?}"),
        }
    }

    #[test]
    fn channel_observer_sends_tool_call_finished() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let observer = ChannelObserver::new(tx);
        let outcome = ToolOutcome {
            content: "ok".to_string(),
            is_error: false,
            truncated: false,
            exit: None,
        };

        observer.on_tool_call_finished("call-1", &outcome);

        match rx.try_recv().unwrap() {
            AgentEvent::ToolCallFinished {
                id,
                outcome: actual,
            } => {
                assert_eq!(id, "call-1");
                assert_eq!(actual, outcome);
            }
            other => panic!("expected ToolCallFinished, got {other:?}"),
        }
    }

    #[test]
    fn channel_observer_forwards_only_root_scoped_statuses() {
        let (root, child) = observer_identities();
        let (tx, mut rx) = mpsc::unbounded_channel();
        let observer = ChannelObserver::new(tx);

        observer.on_scoped_status(&root, AgentStatus::CallingModel);
        observer.on_scoped_status(&child, AgentStatus::ExecutingTool("read_file".to_string()));

        match rx.try_recv().unwrap() {
            AgentEvent::StatusChanged(status) => assert_eq!(status, AgentStatus::CallingModel),
            other => panic!("expected root StatusChanged, got {other:?}"),
        }
        assert!(
            rx.try_recv().is_err(),
            "child status must not overwrite the root TUI status"
        );
    }

    #[test]
    fn channel_observer_ignores_child_tool_events_even_with_duplicate_outer_call_id() {
        let (root, child) = observer_identities();
        let (tx, mut rx) = mpsc::unbounded_channel();
        let observer = ChannelObserver::new(tx);
        let root_outcome = ToolOutcome {
            content: "outer complete".to_string(),
            is_error: false,
            truncated: false,
            exit: None,
        };
        let child_outcome = ToolOutcome {
            content: "child complete".to_string(),
            is_error: false,
            truncated: false,
            exit: None,
        };

        observer.on_scoped_tool_call_started(
            &root,
            "duplicate-id",
            "delegate_task",
            &json!({ "task": "inspect" }),
            true,
        );
        observer.on_scoped_tool_call_started(
            &child,
            "duplicate-id",
            "read_file",
            &json!({ "path": "README.md" }),
            true,
        );
        observer.on_scoped_tool_call_finished(&child, "duplicate-id", &child_outcome);
        observer.on_scoped_tool_call_finished(&root, "duplicate-id", &root_outcome);

        match rx.try_recv().unwrap() {
            AgentEvent::ToolCallStarted { id, name, .. } => {
                assert_eq!(id, "duplicate-id");
                assert_eq!(name, "delegate_task");
            }
            other => panic!("expected outer ToolCallStarted, got {other:?}"),
        }
        match rx.try_recv().unwrap() {
            AgentEvent::ToolCallFinished { id, outcome } => {
                assert_eq!(id, "duplicate-id");
                assert_eq!(outcome, root_outcome);
            }
            other => panic!(
                "child duplicate id must not close or replace the outer tool card, got {other:?}"
            ),
        }
        assert!(
            rx.try_recv().is_err(),
            "only the root start/finish pair may reach TUI"
        );
    }

    #[test]
    fn channel_observer_aggregates_root_and_child_scoped_usage() {
        let (root, child) = observer_identities();
        let (tx, mut rx) = mpsc::unbounded_channel();
        let observer = ChannelObserver::new(tx);

        observer.on_scoped_usage(
            &root,
            &Usage {
                input_tokens: 2,
                output_tokens: 3,
            },
        );
        observer.on_scoped_usage(
            &child,
            &Usage {
                input_tokens: 5,
                output_tokens: 7,
            },
        );

        match rx.try_recv().unwrap() {
            AgentEvent::Usage {
                input_tokens,
                output_tokens,
            } => assert_eq!((input_tokens, output_tokens), (2, 3)),
            other => panic!("expected root Usage, got {other:?}"),
        }
        match rx.try_recv().unwrap() {
            AgentEvent::Usage {
                input_tokens,
                output_tokens,
            } => assert_eq!((input_tokens, output_tokens), (5, 7)),
            other => panic!("expected child Usage, got {other:?}"),
        }
        assert!(rx.try_recv().is_err());
    }

    struct ExecuteTool;

    #[async_trait]
    impl Tool for ExecuteTool {
        fn name(&self) -> &str {
            "execute_tool"
        }

        fn description(&self) -> &str {
            "Requires execute confirmation"
        }

        fn schema(&self) -> Value {
            json!({ "type": "object" })
        }

        fn permission_level(&self) -> PermissionLevel {
            PermissionLevel::Execute
        }

        async fn execute(&self, _args: Value, _ctx: &ToolContext) -> ToolOutcome {
            ToolOutcome {
                content: "ok".to_string(),
                is_error: false,
                truncated: false,
                exit: None,
            }
        }
    }

    struct EditTool;

    #[async_trait]
    impl Tool for EditTool {
        fn name(&self) -> &str {
            "edit_tool"
        }

        fn description(&self) -> &str {
            "Requires edit confirmation"
        }

        fn schema(&self) -> Value {
            json!({ "type": "object" })
        }

        fn permission_level(&self) -> PermissionLevel {
            PermissionLevel::Edit
        }

        async fn execute(&self, _args: Value, _ctx: &ToolContext) -> ToolOutcome {
            ToolOutcome {
                content: "ok".to_string(),
                is_error: false,
                truncated: false,
                exit: None,
            }
        }
    }

    struct NetworkTool;

    #[async_trait]
    impl Tool for NetworkTool {
        fn name(&self) -> &str {
            "network_tool"
        }

        fn description(&self) -> &str {
            "Requires network permission"
        }

        fn schema(&self) -> Value {
            json!({ "type": "object" })
        }

        fn permission_level(&self) -> PermissionLevel {
            PermissionLevel::Network
        }

        async fn execute(&self, _args: Value, _ctx: &ToolContext) -> ToolOutcome {
            ToolOutcome {
                content: "ok".to_string(),
                is_error: false,
                truncated: false,
                exit: None,
            }
        }
    }

    fn authorizable_network_preview() -> NetworkPermissionPreview {
        NetworkPermissionPreview {
            authorizable: true,
            full_args: json!({ "url": "https://example.com" }),
            canonical_initial_target: Some("https://example.com/".to_string()),
            scope: Some(NetworkPermissionScope {
                max_redirects: 3,
                may_cross_origin: true,
                ssrf_each_hop: true,
            }),
            denial_reason: None,
        }
    }

    fn reject_only_network_preview() -> NetworkPermissionPreview {
        NetworkPermissionPreview {
            authorizable: false,
            full_args: json!({ "url": "bad" }),
            canonical_initial_target: None,
            scope: None,
            denial_reason: Some("invalid network target".to_string()),
        }
    }

    fn network_call() -> ToolCall {
        ToolCall {
            id: "call-network".to_string(),
            name: "network_tool".to_string(),
            arguments: json!({ "url": "https://example.com" }),
        }
    }

    fn execute_call() -> ToolCall {
        ToolCall {
            id: "call-exec".to_string(),
            name: "execute_tool".to_string(),
            arguments: json!({ "command": "echo hi" }),
        }
    }

    fn edit_call() -> ToolCall {
        ToolCall {
            id: "call-edit".to_string(),
            name: "edit_tool".to_string(),
            arguments: json!({ "path": "note.txt" }),
        }
    }

    fn decider_with_mode(
        mode: PermissionMode,
    ) -> (
        ChannelDecider,
        mpsc::UnboundedReceiver<AgentEvent>,
        Arc<Mutex<PermissionMode>>,
    ) {
        decider_with_mode_and_policy(mode, [])
    }

    fn decider_with_mode_and_policy<const N: usize>(
        mode: PermissionMode,
        allowed: [&str; N],
    ) -> (
        ChannelDecider,
        mpsc::UnboundedReceiver<AgentEvent>,
        Arc<Mutex<PermissionMode>>,
    ) {
        decider_with_mode_policy_and_path(mode, allowed, PathBuf::from("user-config.toml"))
    }

    fn decider_with_mode_policy_and_path<const N: usize>(
        mode: PermissionMode,
        allowed: [&str; N],
        user_config_path: PathBuf,
    ) -> (
        ChannelDecider,
        mpsc::UnboundedReceiver<AgentEvent>,
        Arc<Mutex<PermissionMode>>,
    ) {
        let (tx, rx) = mpsc::unbounded_channel();
        let mode = Arc::new(Mutex::new(mode));
        (
            ChannelDecider::new(
                tx,
                mode.clone(),
                PolicyEngine::from_commands(allowed),
                user_config_path,
            ),
            rx,
            mode,
        )
    }

    // --- §2.1 ChannelDecider + PermissionMode(卡点 B) ---

    #[tokio::test]
    async fn channel_decider_yolo_auto_allows_execute_without_channel() {
        let (decider, mut rx, _mode) = decider_with_mode(PermissionMode::Yolo);
        let call = execute_call();
        let tool = ExecuteTool;

        let decision = timeout(
            Duration::from_millis(50),
            decider.decide(permission_check(&call, &tool)),
        )
        .await
        .expect("Yolo + Execute must return Allow immediately without channel round-trip");

        assert_eq!(decision, PermissionDecision::Allow);
        assert!(rx.try_recv().is_err(), "must not send PermissionRequired");
    }

    #[tokio::test]
    async fn channel_decider_accept_edits_auto_allows_edit_without_channel() {
        let (decider, mut rx, _mode) = decider_with_mode(PermissionMode::AcceptEdits);
        let call = edit_call();
        let tool = EditTool;

        let decision = timeout(
            Duration::from_millis(50),
            decider.decide(permission_check(&call, &tool)),
        )
        .await
        .expect("AcceptEdits + Edit must return Allow immediately without channel round-trip");

        assert_eq!(decision, PermissionDecision::Allow);
        assert!(rx.try_recv().is_err(), "must not send PermissionRequired");
    }

    #[tokio::test]
    async fn channel_decider_accept_edits_still_asks_for_execute_via_channel() {
        let (decider, mut rx, _mode) = decider_with_mode(PermissionMode::AcceptEdits);
        let call = execute_call();
        let tool = ExecuteTool;
        let decision = decider.decide(permission_check(&call, &tool));
        tokio::pin!(decision);

        let request = tokio::select! {
            event = rx.recv() => event.expect("permission request should be sent"),
            decision = &mut decision => panic!("decide returned before permission response: {decision:?}"),
            _ = sleep(Duration::from_millis(50)) => panic!("timed out waiting for permission request"),
        };

        match request {
            AgentEvent::PermissionRequired(request) => {
                assert_eq!(request.tool_name, "execute_tool");
                assert_eq!(request.allow_always_key.as_deref(), Some("echo hi"));
                request.responder.send(PermissionReply::AllowOnce).unwrap();
            }
            other => panic!("expected PermissionRequired, got {other:?}"),
        }

        assert_eq!(decision.await, PermissionDecision::Allow);
    }

    #[tokio::test]
    async fn channel_decider_sends_the_gate_network_preview_in_non_yolo_modes() {
        for mode in [
            PermissionMode::Normal,
            PermissionMode::AcceptEdits,
            PermissionMode::Plan,
        ] {
            let (decider, mut rx, _mode) = decider_with_mode(mode);
            let call = network_call();
            let tool = NetworkTool;
            let preview = authorizable_network_preview();
            let decision = decider.decide(network_permission_check(&call, &tool, &preview));
            tokio::pin!(decision);

            let event = tokio::select! {
                event = rx.recv() => event.expect("network permission request"),
                decision = &mut decision => panic!("decide returned before permission request: {decision:?}"),
                _ = sleep(Duration::from_millis(50)) => panic!("timed out waiting for permission request"),
            };
            match event {
                AgentEvent::PermissionRequired(request) => {
                    assert_eq!(
                        request.permission_level,
                        PermissionLevel::Network,
                        "{mode:?}"
                    );
                    assert_eq!(request.network_preview, Some(preview.clone()));
                    assert_eq!(request.allow_always_key, None);
                    request.responder.send(PermissionReply::AllowOnce).unwrap();
                }
                other => panic!("expected PermissionRequired, got {other:?}"),
            }
            assert_eq!(decision.await, PermissionDecision::Allow);
        }
    }

    #[tokio::test]
    async fn channel_decider_network_allow_once_reprompts_same_target() {
        let (decider, mut rx, _mode) = decider_with_mode(PermissionMode::Normal);
        let call = network_call();
        let tool = NetworkTool;
        let preview = authorizable_network_preview();

        for attempt in 1..=2 {
            let decision = decider.decide(network_permission_check(&call, &tool, &preview));
            tokio::pin!(decision);
            let event = tokio::select! {
                event = rx.recv() => event.expect("network permission request"),
                decision = &mut decision => panic!("attempt {attempt} returned before permission request: {decision:?}"),
                _ = sleep(Duration::from_millis(50)) => panic!("attempt {attempt} did not request permission"),
            };
            match event {
                AgentEvent::PermissionRequired(request) => {
                    assert_eq!(request.permission_level, PermissionLevel::Network);
                    assert_eq!(request.allow_always_key, None);
                    request.responder.send(PermissionReply::AllowOnce).unwrap();
                }
                other => panic!("expected PermissionRequired, got {other:?}"),
            }
            assert_eq!(decision.await, PermissionDecision::Allow);
        }
    }

    #[tokio::test]
    async fn channel_decider_reject_only_network_never_auto_allows_or_accepts_allow_reply() {
        for mode in [
            PermissionMode::Normal,
            PermissionMode::AcceptEdits,
            PermissionMode::Yolo,
            PermissionMode::Plan,
        ] {
            let (decider, mut rx, _mode) = decider_with_mode(mode);
            let call = network_call();
            let tool = NetworkTool;
            let preview = reject_only_network_preview();
            let decision = decider.decide(network_permission_check(&call, &tool, &preview));
            tokio::pin!(decision);

            let event = tokio::select! {
                event = rx.recv() => event.expect("reject-only permission request"),
                decision = &mut decision => panic!("decide returned before reject-only request: {decision:?}"),
                _ = sleep(Duration::from_millis(50)) => panic!("timed out waiting for reject-only permission request"),
            };
            match event {
                AgentEvent::PermissionRequired(request) => {
                    assert_eq!(
                        request.permission_level,
                        PermissionLevel::Network,
                        "{mode:?}"
                    );
                    assert_eq!(request.network_preview, Some(preview.clone()));
                    request.responder.send(PermissionReply::AllowOnce).unwrap();
                }
                other => panic!("expected PermissionRequired, got {other:?}"),
            }
            assert_eq!(decision.await, PermissionDecision::Deny, "{mode:?}");
        }
    }

    #[tokio::test]
    async fn channel_decider_allowlist_hit_returns_allow_without_channel() {
        let (decider, mut rx, _mode) =
            decider_with_mode_and_policy(PermissionMode::Normal, ["echo hi"]);
        let call = execute_call();
        let tool = ExecuteTool;
        let decision = decider.decide(permission_check(&call, &tool));
        tokio::pin!(decision);

        let decision = tokio::select! {
            event = rx.recv() => panic!("must not send PermissionRequired for allowed command: {event:?}"),
            decision = &mut decision => decision,
            _ = sleep(Duration::from_millis(50)) => panic!("timed out waiting for allowlist decision"),
        };

        assert_eq!(decision, PermissionDecision::Allow);
    }

    #[tokio::test]
    async fn channel_decider_normal_miss_sends_permission_request_with_allow_always_key() {
        let (decider, mut rx, _mode) = decider_with_mode(PermissionMode::Normal);
        let call = execute_call();
        let tool = ExecuteTool;
        let decision = decider.decide(permission_check(&call, &tool));
        tokio::pin!(decision);

        let request = tokio::select! {
            event = rx.recv() => event.expect("permission request should be sent"),
            decision = &mut decision => panic!("decide returned before permission response: {decision:?}"),
            _ = sleep(Duration::from_millis(50)) => panic!("timed out waiting for permission request"),
        };

        match request {
            AgentEvent::PermissionRequired(request) => {
                assert_eq!(request.tool_name, "execute_tool");
                assert_eq!(request.allow_always_key.as_deref(), Some("echo hi"));
                drop(request.responder);
            }
            other => panic!("expected PermissionRequired, got {other:?}"),
        }

        assert_eq!(decision.await, PermissionDecision::Deny);
    }

    #[tokio::test]
    async fn channel_decider_allow_always_persists_and_remembers_command() {
        let temp = tempfile::tempdir().unwrap();
        let config_path = temp.path().join("config.toml");
        let (decider, mut rx, _mode) =
            decider_with_mode_policy_and_path(PermissionMode::Normal, [], config_path.clone());
        let call = execute_call();
        let tool = ExecuteTool;
        let decision = decider.decide(permission_check(&call, &tool));
        tokio::pin!(decision);

        let request = tokio::select! {
            event = rx.recv() => event.expect("permission request should be sent"),
            decision = &mut decision => panic!("decide returned before permission response: {decision:?}"),
            _ = sleep(Duration::from_millis(50)) => panic!("timed out waiting for permission request"),
        };
        match request {
            AgentEvent::PermissionRequired(request) => {
                assert_eq!(request.allow_always_key.as_deref(), Some("echo hi"));
                request
                    .responder
                    .send(PermissionReply::AllowAlways)
                    .unwrap();
            }
            other => panic!("expected PermissionRequired, got {other:?}"),
        }

        assert_eq!(decision.await, PermissionDecision::Allow);
        assert_eq!(
            read_raw_config(&config_path).unwrap().allowed_commands,
            Some(vec!["echo hi".to_string()])
        );

        let second = decider.decide(permission_check(&call, &tool));
        tokio::pin!(second);
        let second = tokio::select! {
            event = rx.recv() => panic!("remembered command must not send PermissionRequired: {event:?}"),
            decision = &mut second => decision,
            _ = sleep(Duration::from_millis(50)) => panic!("timed out waiting for remembered command decision"),
        };
        assert_eq!(second, PermissionDecision::Allow);
    }

    #[tokio::test]
    async fn channel_decider_allow_always_notice_on_persist_failure_but_remembers() {
        let temp = tempfile::tempdir().unwrap();
        let config_path = temp.path().to_path_buf();
        let (decider, mut rx, _mode) =
            decider_with_mode_policy_and_path(PermissionMode::Normal, [], config_path);
        let call = execute_call();
        let tool = ExecuteTool;
        let decision = decider.decide(permission_check(&call, &tool));
        tokio::pin!(decision);

        let request = tokio::select! {
            event = rx.recv() => event.expect("permission request should be sent"),
            decision = &mut decision => panic!("decide returned before permission response: {decision:?}"),
            _ = sleep(Duration::from_millis(50)) => panic!("timed out waiting for permission request"),
        };
        match request {
            AgentEvent::PermissionRequired(request) => {
                request
                    .responder
                    .send(PermissionReply::AllowAlways)
                    .unwrap();
            }
            other => panic!("expected PermissionRequired, got {other:?}"),
        }

        assert_eq!(decision.await, PermissionDecision::Allow);
        let notice = tokio::select! {
            event = rx.recv() => event.expect("persist failure notice"),
            _ = sleep(Duration::from_millis(50)) => panic!("timed out waiting for persist failure notice"),
        };
        match notice {
            AgentEvent::Notice(message) => {
                assert!(message.starts_with("命令白名单持久化失败:"));
            }
            other => panic!("expected Notice, got {other:?}"),
        }

        let second = decider.decide(permission_check(&call, &tool));
        tokio::pin!(second);
        let second = tokio::select! {
            event = rx.recv() => panic!("remembered command must not send PermissionRequired: {event:?}"),
            decision = &mut second => decision,
            _ = sleep(Duration::from_millis(50)) => panic!("timed out waiting for remembered command decision"),
        };
        assert_eq!(second, PermissionDecision::Allow);
    }

    #[tokio::test]
    async fn channel_decider_allow_always_without_key_does_not_persist() {
        let temp = tempfile::tempdir().unwrap();
        let config_path = temp.path().join("config.toml");
        let (decider, mut rx, _mode) =
            decider_with_mode_policy_and_path(PermissionMode::Normal, [], config_path.clone());
        let call = call();
        let tool = ConfirmTool;
        let decision = decider.decide(permission_check(&call, &tool));
        tokio::pin!(decision);

        let request = tokio::select! {
            event = rx.recv() => event.expect("permission request should be sent"),
            decision = &mut decision => panic!("decide returned before permission response: {decision:?}"),
            _ = sleep(Duration::from_millis(50)) => panic!("timed out waiting for permission request"),
        };
        match request {
            AgentEvent::PermissionRequired(request) => {
                assert_eq!(request.allow_always_key, None);
                request
                    .responder
                    .send(PermissionReply::AllowAlways)
                    .unwrap();
            }
            other => panic!("expected PermissionRequired, got {other:?}"),
        }

        assert_eq!(decision.await, PermissionDecision::Allow);
        assert!(
            !config_path.exists(),
            "keyless AllowAlways must not create or update config"
        );
    }

    struct ConfirmTool;

    #[async_trait]
    impl Tool for ConfirmTool {
        fn name(&self) -> &str {
            "confirm_tool"
        }

        fn description(&self) -> &str {
            "Requires confirmation"
        }

        fn schema(&self) -> Value {
            json!({ "type": "object" })
        }

        fn permission_level(&self) -> PermissionLevel {
            PermissionLevel::Execute
        }

        async fn execute(&self, _args: Value, _ctx: &ToolContext) -> ToolOutcome {
            ToolOutcome {
                content: "ok".to_string(),
                is_error: false,
                truncated: false,
                exit: None,
            }
        }
    }

    fn call() -> ToolCall {
        ToolCall {
            id: "call-1".to_string(),
            name: "confirm_tool".to_string(),
            arguments: json!({ "path": "note.txt" }),
        }
    }

    #[tokio::test]
    async fn channel_decider_returns_allow_from_permission_responder() {
        let (decider, mut rx, _mode) = decider_with_mode(PermissionMode::Normal);
        let call = call();
        let tool = ConfirmTool;
        let decision = decider.decide(permission_check(&call, &tool));
        tokio::pin!(decision);

        let request = tokio::select! {
            event = rx.recv() => event.expect("permission request should be sent"),
            decision = &mut decision => panic!("decide returned before permission response: {decision:?}"),
            _ = sleep(Duration::from_millis(50)) => panic!("timed out waiting for permission request"),
        };

        match request {
            AgentEvent::PermissionRequired(request) => {
                assert_eq!(request.tool_name, "confirm_tool");
                assert_eq!(request.args, json!({ "path": "note.txt" }));
                request.responder.send(PermissionReply::AllowOnce).unwrap();
            }
            other => panic!("expected PermissionRequired, got {other:?}"),
        }

        assert_eq!(decision.await, PermissionDecision::Allow);
    }

    #[tokio::test]
    async fn channel_decider_denies_when_permission_responder_is_dropped() {
        let (decider, mut rx, _mode) = decider_with_mode(PermissionMode::Normal);
        let call = call();
        let tool = ConfirmTool;
        let decision = decider.decide(permission_check(&call, &tool));
        tokio::pin!(decision);

        let request = tokio::select! {
            event = rx.recv() => event.expect("permission request should be sent"),
            decision = &mut decision => panic!("decide returned before permission response: {decision:?}"),
            _ = sleep(Duration::from_millis(50)) => panic!("timed out waiting for permission request"),
        };

        match request {
            AgentEvent::PermissionRequired(request) => drop(request.responder),
            other => panic!("expected PermissionRequired, got {other:?}"),
        }

        assert_eq!(decision.await, PermissionDecision::Deny);
    }

    #[tokio::test]
    async fn channel_plan_approver_rejects_when_responder_is_dropped() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let approver = ChannelPlanApprover::new(tx, Arc::new(Mutex::new(PermissionMode::Plan)));
        let plan = Plan {
            title: "Test".to_string(),
            steps: vec![],
        };
        let decision = approver.approve(&plan);
        tokio::pin!(decision);

        let request = tokio::select! {
            event = rx.recv() => event.expect("plan approval request should be sent"),
            decision = &mut decision => panic!("approve returned before UI response: {decision:?}"),
            _ = sleep(Duration::from_millis(50)) => panic!("timed out waiting for plan approval request"),
        };

        match request {
            AgentEvent::PlanApprovalRequired(request) => drop(request.responder),
            other => panic!("expected PlanApprovalRequired, got {other:?}"),
        }

        assert!(matches!(
            decision.await,
            PlanDecision::Reject(reason) if reason.contains("disconnected")
        ));
    }

    #[tokio::test]
    async fn channel_plan_approver_flips_mode_after_approve() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let mode = Arc::new(Mutex::new(PermissionMode::Plan));
        let approver = ChannelPlanApprover::new(tx, mode.clone());
        let plan = Plan {
            title: "Test".to_string(),
            steps: vec![],
        };
        let decision = approver.approve(&plan);
        tokio::pin!(decision);

        let request = tokio::select! {
            event = rx.recv() => event.expect("plan approval request should be sent"),
            decision = &mut decision => panic!("approve returned before UI response: {decision:?}"),
            _ = sleep(Duration::from_millis(50)) => panic!("timed out waiting for plan approval request"),
        };
        match request {
            AgentEvent::PlanApprovalRequired(request) => {
                request.responder.send(PlanDecision::Approve).unwrap();
            }
            other => panic!("expected PlanApprovalRequired, got {other:?}"),
        }

        assert_eq!(decision.await, PlanDecision::Approve);
        assert_eq!(*mode.lock().unwrap(), PermissionMode::AcceptEdits);
    }

    #[tokio::test]
    async fn channel_prompter_returns_empty_answer_when_responder_is_dropped() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let prompter = ChannelPrompter::new(tx);
        let question = Question {
            question: "Pick one".to_string(),
            options: vec![],
            allow_multi: false,
            allow_other: false,
        };
        let answer = prompter.prompt(&question);
        tokio::pin!(answer);

        let request = tokio::select! {
            event = rx.recv() => event.expect("question request should be sent"),
            answer = &mut answer => panic!("prompt returned before UI response: {answer:?}"),
            _ = sleep(Duration::from_millis(50)) => panic!("timed out waiting for question request"),
        };
        match request {
            AgentEvent::UserQuestionRequired(request) => drop(request.responder),
            other => panic!("expected UserQuestionRequired, got {other:?}"),
        }

        let answer = answer.await;
        assert!(answer.selected.is_empty());
        assert!(answer.supplement.is_none());
    }

    #[test]
    fn channel_progress_reporter_sends_plan_progress_event() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let reporter = ChannelProgressReporter::new(tx);
        let update = PlanProgressUpdate {
            step: 2,
            status: StepStatus::Done,
            validation_result: Some("cargo test → 12 passed".to_string()),
        };

        reporter.report(update.clone());

        match rx.try_recv().unwrap() {
            AgentEvent::PlanProgress(received) => assert_eq!(received, update),
            other => panic!("expected PlanProgress, got {other:?}"),
        }
    }

    #[test]
    fn channel_progress_reporter_does_not_panic_when_sender_dropped() {
        let (tx, rx) = mpsc::unbounded_channel();
        drop(rx);
        let reporter = ChannelProgressReporter::new(tx);
        reporter.report(PlanProgressUpdate {
            step: 1,
            status: StepStatus::InProgress,
            validation_result: None,
        });
    }
}
