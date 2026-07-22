use crate::agent::{AgentExecutionScope, ExecutionCapabilities};
use crate::provider::ToolCall;
use crate::tool::{NetworkPermissionPreview, PermissionLevel, Tool};
use async_trait::async_trait;
use std::collections::BTreeSet;
use thiserror::Error;

pub mod preview;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PermissionDecision {
    Allow,
    Deny,
}

pub struct PermissionCheck<'a> {
    pub call: &'a ToolCall,
    pub tool: &'a dyn Tool,
    pub network_preview: Option<&'a NetworkPermissionPreview>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PermissionDenial {
    UserDenied,
    NetworkUnauthorizable(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PermissionGateOutcome {
    Allow,
    Deny(PermissionDenial),
}

#[derive(Clone, Debug, Error, PartialEq, Eq)]
#[error("execution scope violation: {reason}")]
pub struct ScopeViolation {
    reason: String,
}

impl ScopeViolation {
    pub fn new(reason: impl Into<String>) -> Self {
        Self {
            reason: reason.into(),
        }
    }

    pub fn reason(&self) -> &str {
        &self.reason
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PermissionReply {
    AllowOnce,
    AllowAlways,
    Deny,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PolicyEngine {
    allowed: BTreeSet<String>,
}

pub fn normalize(cmd: &str) -> String {
    cmd.split_whitespace().collect::<Vec<_>>().join(" ")
}

impl PolicyEngine {
    pub fn from_commands<I, S>(commands: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let allowed = commands
            .into_iter()
            .map(|command| normalize(command.as_ref()))
            .filter(|command| !command.is_empty())
            .collect();
        Self { allowed }
    }

    pub fn permission_key(call: &ToolCall, tool: &dyn Tool) -> Option<String> {
        if !matches!(tool.permission_level(), PermissionLevel::Execute) {
            return None;
        }
        call.arguments
            .get("command")
            .and_then(|command| command.as_str())
            .map(normalize)
            .filter(|key| !key.is_empty())
    }

    pub fn is_allowed(&self, call: &ToolCall, tool: &dyn Tool) -> bool {
        Self::permission_key(call, tool).is_some_and(|key| self.allowed.contains(&key))
    }

    pub fn remember(&mut self, key: String) {
        let key = normalize(&key);
        if !key.is_empty() {
            self.allowed.insert(key);
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PermissionMode {
    Normal,
    AcceptEdits,
    Yolo,
    Plan,
}

pub fn auto_allows(mode: PermissionMode, level: PermissionLevel) -> bool {
    match (mode, level) {
        (PermissionMode::Yolo, PermissionLevel::Network) => true,
        (
            PermissionMode::Normal | PermissionMode::AcceptEdits | PermissionMode::Plan,
            PermissionLevel::Network,
        ) => false,
        (PermissionMode::Normal, PermissionLevel::Edit | PermissionLevel::Execute) => false,
        (PermissionMode::AcceptEdits, PermissionLevel::Edit) => true,
        (PermissionMode::AcceptEdits, PermissionLevel::Execute) => false,
        (PermissionMode::Yolo, PermissionLevel::Edit | PermissionLevel::Execute) => true,
        (PermissionMode::Plan, PermissionLevel::Edit | PermissionLevel::Execute) => false,
        (_, PermissionLevel::ReadOnly) => false,
    }
}

pub fn cycle_permission_mode(mode: PermissionMode) -> PermissionMode {
    match mode {
        PermissionMode::Normal => PermissionMode::AcceptEdits,
        PermissionMode::AcceptEdits => PermissionMode::Yolo,
        PermissionMode::Yolo => PermissionMode::Plan,
        PermissionMode::Plan => PermissionMode::Normal,
    }
}

pub fn permission_mode_label(mode: PermissionMode) -> &'static str {
    match mode {
        PermissionMode::Normal => "normal",
        PermissionMode::AcceptEdits => "accept-edits",
        PermissionMode::Yolo => "yolo",
        PermissionMode::Plan => "plan",
    }
}

#[async_trait]
pub trait PermissionDecider: Send + Sync {
    async fn decide(&self, check: PermissionCheck<'_>) -> PermissionDecision;
}

pub async fn gate(
    call: &ToolCall,
    tool: &dyn Tool,
    decider: &dyn PermissionDecider,
) -> PermissionGateOutcome {
    match tool.permission_level() {
        PermissionLevel::ReadOnly => PermissionGateOutcome::Allow,
        PermissionLevel::Network => {
            let preview = tool.network_permission_preview(&call.arguments);
            let denial_reason = network_preview_denial_reason(&preview);
            let decision = decider
                .decide(PermissionCheck {
                    call,
                    tool,
                    network_preview: Some(&preview),
                })
                .await;

            match (denial_reason, decision) {
                (Some(reason), _) => {
                    PermissionGateOutcome::Deny(PermissionDenial::NetworkUnauthorizable(reason))
                }
                (None, PermissionDecision::Allow) => PermissionGateOutcome::Allow,
                (None, PermissionDecision::Deny) => {
                    PermissionGateOutcome::Deny(PermissionDenial::UserDenied)
                }
            }
        }
        PermissionLevel::Edit | PermissionLevel::Execute => {
            match decider
                .decide(PermissionCheck {
                    call,
                    tool,
                    network_preview: None,
                })
                .await
            {
                PermissionDecision::Allow => PermissionGateOutcome::Allow,
                PermissionDecision::Deny => {
                    PermissionGateOutcome::Deny(PermissionDenial::UserDenied)
                }
            }
        }
    }
}

pub async fn gate_scoped(
    call: &ToolCall,
    tool: &dyn Tool,
    decider: &dyn PermissionDecider,
    capabilities: &ExecutionCapabilities,
) -> Result<PermissionGateOutcome, ScopeViolation> {
    ensure_tool_in_scope(tool, capabilities)?;
    Ok(gate(call, tool, decider).await)
}

pub fn ensure_tool_in_scope(
    tool: &dyn Tool,
    capabilities: &ExecutionCapabilities,
) -> Result<(), ScopeViolation> {
    if !capabilities.tool_names().contains(tool.name()) {
        return Err(ScopeViolation::new(format!(
            "tool `{}` is outside the execution scope",
            tool.name()
        )));
    }
    if !capabilities
        .permission_levels()
        .contains(&tool.permission_level())
    {
        return Err(ScopeViolation::new(format!(
            "permission level `{:?}` for tool `{}` is outside the execution scope",
            tool.permission_level(),
            tool.name()
        )));
    }

    Ok(())
}

pub fn ensure_tool_in_execution_scope(
    tool: &dyn Tool,
    scope: &AgentExecutionScope,
) -> Result<(), ScopeViolation> {
    ensure_tool_in_scope(tool, scope.capabilities())?;

    let required_child_depth = tool.required_child_depth();
    let remaining_child_depth = scope.budget().remaining_child_depth;
    if required_child_depth > remaining_child_depth {
        return Err(ScopeViolation::new(format!(
            "tool `{}` requires child depth {}, but execution scope has {} remaining",
            tool.name(),
            required_child_depth,
            remaining_child_depth
        )));
    }

    Ok(())
}

fn network_preview_denial_reason(preview: &NetworkPermissionPreview) -> Option<String> {
    if !preview.authorizable {
        return Some(
            preview
                .denial_reason
                .clone()
                .filter(|reason| !reason.is_empty())
                .unwrap_or_else(|| "network preview is not authorizable".to_string()),
        );
    }

    if preview.canonical_initial_target.is_none() {
        return Some("network preview is missing a canonical initial target".to_string());
    }

    if preview.scope.is_none() {
        return Some("network preview is missing a permission scope".to_string());
    }

    preview
        .denial_reason
        .as_deref()
        .filter(|reason| !reason.is_empty())
        .map(|_| "authorizable network preview contains a denial reason".to_string())
}

#[cfg(test)]
mod tests {
    use super::{
        auto_allows, cycle_permission_mode, gate, gate_scoped, normalize, permission_mode_label,
        PermissionCheck, PermissionDecider, PermissionDecision, PermissionDenial,
        PermissionGateOutcome, PermissionMode, PolicyEngine,
    };
    use crate::agent::ExecutionCapabilities;
    use crate::provider::ToolCall;
    use crate::tool::{
        NetworkPermissionPreview, NetworkPermissionScope, PermissionLevel, Tool, ToolContext,
        ToolOutcome,
    };
    use async_trait::async_trait;
    use serde_json::{json, Value};
    use std::collections::BTreeSet;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;

    struct MockTool {
        permission_level: PermissionLevel,
    }

    #[async_trait]
    impl Tool for MockTool {
        fn name(&self) -> &str {
            "mock_tool"
        }

        fn description(&self) -> &str {
            "Mock tool"
        }

        fn schema(&self) -> Value {
            json!({ "type": "object" })
        }

        fn permission_level(&self) -> PermissionLevel {
            self.permission_level.clone()
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

    struct PreviewTool {
        preview: NetworkPermissionPreview,
        preview_calls: AtomicUsize,
    }

    impl PreviewTool {
        fn new(preview: NetworkPermissionPreview) -> Self {
            Self {
                preview,
                preview_calls: AtomicUsize::new(0),
            }
        }

        fn preview_calls(&self) -> usize {
            self.preview_calls.load(Ordering::SeqCst)
        }
    }

    #[async_trait]
    impl Tool for PreviewTool {
        fn name(&self) -> &str {
            "network_tool"
        }

        fn description(&self) -> &str {
            "Network tool"
        }

        fn schema(&self) -> Value {
            json!({ "type": "object" })
        }

        fn permission_level(&self) -> PermissionLevel {
            PermissionLevel::Network
        }

        fn network_permission_preview(&self, _args: &Value) -> NetworkPermissionPreview {
            self.preview_calls.fetch_add(1, Ordering::SeqCst);
            self.preview.clone()
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

    struct StaticDecider {
        decision: PermissionDecision,
        calls: AtomicUsize,
    }

    impl StaticDecider {
        fn new(decision: PermissionDecision) -> Self {
            Self {
                decision,
                calls: AtomicUsize::new(0),
            }
        }

        fn calls(&self) -> usize {
            self.calls.load(Ordering::SeqCst)
        }
    }

    #[async_trait]
    impl PermissionDecider for StaticDecider {
        async fn decide(&self, check: PermissionCheck<'_>) -> PermissionDecision {
            self.calls.fetch_add(1, Ordering::SeqCst);
            assert_eq!(check.call.name, "mock_tool");
            assert_eq!(check.tool.name(), "mock_tool");
            self.decision.clone()
        }
    }

    struct RecordingDecider {
        decision: PermissionDecision,
        calls: AtomicUsize,
        seen_preview: Mutex<Option<NetworkPermissionPreview>>,
    }

    impl RecordingDecider {
        fn new(decision: PermissionDecision) -> Self {
            Self {
                decision,
                calls: AtomicUsize::new(0),
                seen_preview: Mutex::new(None),
            }
        }

        fn calls(&self) -> usize {
            self.calls.load(Ordering::SeqCst)
        }

        fn seen_preview(&self) -> Option<NetworkPermissionPreview> {
            self.seen_preview
                .lock()
                .expect("preview mutex poisoned")
                .clone()
        }
    }

    #[async_trait]
    impl PermissionDecider for RecordingDecider {
        async fn decide(&self, check: PermissionCheck<'_>) -> PermissionDecision {
            self.calls.fetch_add(1, Ordering::SeqCst);
            *self.seen_preview.lock().expect("preview mutex poisoned") =
                check.network_preview.cloned();
            self.decision.clone()
        }
    }

    fn call() -> ToolCall {
        ToolCall {
            id: "call-1".to_string(),
            name: "mock_tool".to_string(),
            arguments: json!({}),
        }
    }

    fn command_call(command: Value) -> ToolCall {
        ToolCall {
            id: "call-command".to_string(),
            name: "mock_tool".to_string(),
            arguments: json!({ "command": command }),
        }
    }

    fn network_call() -> ToolCall {
        ToolCall {
            id: "call-network".to_string(),
            name: "network_tool".to_string(),
            arguments: json!({ "url": "https://example.com" }),
        }
    }

    fn authorizable_preview() -> NetworkPermissionPreview {
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

    fn unauthorizable_preview(reason: &str) -> NetworkPermissionPreview {
        NetworkPermissionPreview {
            authorizable: false,
            full_args: json!({ "url": "not-a-url" }),
            canonical_initial_target: None,
            scope: None,
            denial_reason: Some(reason.to_string()),
        }
    }

    fn capabilities(
        tool_names: &[&str],
        permission_levels: &[PermissionLevel],
    ) -> ExecutionCapabilities {
        ExecutionCapabilities::try_new(
            tool_names.iter().copied(),
            permission_levels.iter().cloned(),
        )
        .unwrap()
    }

    fn assert_network_unauthorizable(outcome: PermissionGateOutcome) {
        match outcome {
            PermissionGateOutcome::Deny(PermissionDenial::NetworkUnauthorizable(reason)) => {
                assert!(!reason.is_empty());
            }
            other => panic!("expected NetworkUnauthorizable, got {other:?}"),
        }
    }

    #[test]
    fn policy_normalize_trims_and_compresses_internal_whitespace() {
        assert_eq!(normalize("  git   status\t-s\n"), "git status -s");
        assert_eq!(normalize("   "), "");
    }

    #[test]
    fn policy_permission_key_returns_normalized_command_for_execute_tools() {
        let tool = MockTool {
            permission_level: PermissionLevel::Execute,
        };

        assert_eq!(
            PolicyEngine::permission_key(&command_call(json!("cargo   build")), &tool),
            Some("cargo build".to_string())
        );
    }

    #[test]
    fn policy_permission_key_ignores_non_execute_or_missing_command() {
        let edit_tool = MockTool {
            permission_level: PermissionLevel::Edit,
        };
        let readonly_tool = MockTool {
            permission_level: PermissionLevel::ReadOnly,
        };
        let execute_tool = MockTool {
            permission_level: PermissionLevel::Execute,
        };
        let network_tool = MockTool {
            permission_level: PermissionLevel::Network,
        };

        assert_eq!(
            PolicyEngine::permission_key(&command_call(json!("git status")), &edit_tool),
            None
        );
        assert_eq!(
            PolicyEngine::permission_key(&command_call(json!("git status")), &readonly_tool),
            None
        );
        assert_eq!(PolicyEngine::permission_key(&call(), &execute_tool), None);
        assert_eq!(
            PolicyEngine::permission_key(&command_call(json!(42)), &execute_tool),
            None
        );
        assert_eq!(
            PolicyEngine::permission_key(&command_call(json!("   ")), &execute_tool),
            None
        );
        assert_eq!(
            PolicyEngine::permission_key(
                &command_call(json!("curl https://example.com")),
                &network_tool
            ),
            None
        );
        assert!(
            !PolicyEngine::from_commands(["curl https://example.com"]).is_allowed(
                &command_call(json!("curl https://example.com")),
                &network_tool
            )
        );
    }

    #[test]
    fn policy_is_allowed_matches_only_normalized_allowed_commands() {
        let policy = PolicyEngine::from_commands(["git status"]);
        let tool = MockTool {
            permission_level: PermissionLevel::Execute,
        };

        assert!(policy.is_allowed(&command_call(json!("git   status")), &tool));
        assert!(!policy.is_allowed(&command_call(json!("git status -s")), &tool));
    }

    #[test]
    fn policy_remember_adds_command_for_later_checks() {
        let mut policy = PolicyEngine::from_commands(std::iter::empty::<&str>());
        let tool = MockTool {
            permission_level: PermissionLevel::Execute,
        };

        assert!(!policy.is_allowed(&command_call(json!("cargo build")), &tool));
        policy.remember("cargo build".to_string());
        assert!(policy.is_allowed(&command_call(json!("cargo   build")), &tool));
    }

    #[test]
    fn policy_from_commands_normalizes_and_dedups() {
        let policy = PolicyEngine::from_commands([" git  status ", "git status", "cargo build"]);

        assert_eq!(
            policy.allowed,
            BTreeSet::from(["cargo build".to_string(), "git status".to_string()])
        );
    }

    #[tokio::test]
    async fn gate_allows_read_only_tools_without_asking_decider() {
        let tool = MockTool {
            permission_level: PermissionLevel::ReadOnly,
        };
        let decider = StaticDecider::new(PermissionDecision::Deny);

        let decision = gate(&call(), &tool, &decider).await;

        assert_eq!(decision, PermissionGateOutcome::Allow);
        assert_eq!(decider.calls(), 0);
    }

    #[tokio::test]
    async fn scoped_gate_rejects_readonly_tool_name_before_legacy_allow() {
        let tool = MockTool {
            permission_level: PermissionLevel::ReadOnly,
        };
        let decider = StaticDecider::new(PermissionDecision::Allow);
        let restricted = capabilities(&["different_tool"], &[PermissionLevel::ReadOnly]);

        let result = gate_scoped(&call(), &tool, &decider, &restricted).await;

        let violation = result.unwrap_err();
        assert_eq!(decider.calls(), 0);
        assert!(violation.reason().contains("mock_tool"));
        assert!(!violation.reason().contains("secret"));
    }

    #[tokio::test]
    async fn scoped_gate_rejects_execute_level_before_always_allow_decider() {
        let tool = MockTool {
            permission_level: PermissionLevel::Execute,
        };
        let decider = StaticDecider::new(PermissionDecision::Allow);
        let restricted = capabilities(&["mock_tool"], &[PermissionLevel::ReadOnly]);

        let result = gate_scoped(&call(), &tool, &decider, &restricted).await;

        assert_eq!(decider.calls(), 0);
        let violation = result.unwrap_err();
        assert!(violation.reason().contains("Execute"));
    }

    #[tokio::test]
    async fn scoped_gate_rejects_network_before_preview_and_decider() {
        let tool = PreviewTool::new(authorizable_preview());
        let decider = RecordingDecider::new(PermissionDecision::Allow);
        let restricted = capabilities(&["network_tool"], &[PermissionLevel::ReadOnly]);

        let result = gate_scoped(&network_call(), &tool, &decider, &restricted).await;

        assert_eq!(tool.preview_calls(), 0);
        assert_eq!(decider.calls(), 0);
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn scoped_gate_keeps_allowed_legacy_outcomes_and_denial_variants() {
        let readonly = MockTool {
            permission_level: PermissionLevel::ReadOnly,
        };
        let readonly_decider = StaticDecider::new(PermissionDecision::Deny);
        let readonly_capabilities = capabilities(&["mock_tool"], &[PermissionLevel::ReadOnly]);
        assert_eq!(
            gate_scoped(
                &call(),
                &readonly,
                &readonly_decider,
                &readonly_capabilities
            )
            .await,
            Ok(PermissionGateOutcome::Allow)
        );
        assert_eq!(readonly_decider.calls(), 0);

        let execute = MockTool {
            permission_level: PermissionLevel::Execute,
        };
        let execute_decider = StaticDecider::new(PermissionDecision::Deny);
        let execute_capabilities = capabilities(&["mock_tool"], &[PermissionLevel::Execute]);
        assert_eq!(
            gate_scoped(&call(), &execute, &execute_decider, &execute_capabilities).await,
            Ok(PermissionGateOutcome::Deny(PermissionDenial::UserDenied))
        );
        assert_eq!(execute_decider.calls(), 1);
    }

    #[tokio::test]
    async fn gate_uses_decider_for_edit_and_execute_tools() {
        for level in [PermissionLevel::Edit, PermissionLevel::Execute] {
            let tool = MockTool {
                permission_level: level,
            };
            let allow = StaticDecider::new(PermissionDecision::Allow);
            let deny = StaticDecider::new(PermissionDecision::Deny);

            assert_eq!(
                gate(&call(), &tool, &allow).await,
                PermissionGateOutcome::Allow
            );
            assert_eq!(allow.calls(), 1);
            assert_eq!(
                gate(&call(), &tool, &deny).await,
                PermissionGateOutcome::Deny(PermissionDenial::UserDenied)
            );
            assert_eq!(deny.calls(), 1);
        }
    }

    #[tokio::test]
    async fn gate_passes_no_network_preview_for_edit_and_execute() {
        for level in [PermissionLevel::Edit, PermissionLevel::Execute] {
            let tool = MockTool {
                permission_level: level,
            };
            let decider = RecordingDecider::new(PermissionDecision::Allow);

            assert_eq!(
                gate(&call(), &tool, &decider).await,
                PermissionGateOutcome::Allow
            );
            assert_eq!(decider.calls(), 1);
            assert_eq!(decider.seen_preview(), None);
        }
    }

    #[tokio::test]
    async fn network_gate_forwards_one_authorizable_preview_to_decider() {
        let preview = authorizable_preview();
        let tool = PreviewTool::new(preview.clone());
        let decider = RecordingDecider::new(PermissionDecision::Allow);

        let outcome = gate(&network_call(), &tool, &decider).await;

        assert_eq!(decider.calls(), 1);
        assert_eq!(tool.preview_calls(), 1);
        assert_eq!(decider.seen_preview(), Some(preview));
        assert_eq!(outcome, PermissionGateOutcome::Allow);
    }

    #[tokio::test]
    async fn network_gate_preserves_user_denial_for_authorizable_preview() {
        let tool = PreviewTool::new(authorizable_preview());
        let decider = RecordingDecider::new(PermissionDecision::Deny);

        let outcome = gate(&network_call(), &tool, &decider).await;

        assert_eq!(
            outcome,
            PermissionGateOutcome::Deny(PermissionDenial::UserDenied)
        );
        assert_eq!(decider.calls(), 1);
    }

    #[tokio::test]
    async fn network_gate_preserves_unauthorizable_reason_regardless_of_decider_result() {
        for decision in [PermissionDecision::Allow, PermissionDecision::Deny] {
            let preview = unauthorizable_preview("invalid URL");
            let tool = PreviewTool::new(preview.clone());
            let decider = RecordingDecider::new(decision);

            let outcome = gate(&network_call(), &tool, &decider).await;

            assert_eq!(
                outcome,
                PermissionGateOutcome::Deny(PermissionDenial::NetworkUnauthorizable(
                    "invalid URL".to_string()
                ))
            );
            assert_eq!(decider.calls(), 1);
            assert_eq!(decider.seen_preview(), Some(preview));
            assert_eq!(tool.preview_calls(), 1);
        }
    }

    #[tokio::test]
    async fn network_gate_rejects_malformed_authorizable_preview() {
        for preview in [
            NetworkPermissionPreview {
                authorizable: true,
                full_args: json!({ "url": "https://example.com" }),
                canonical_initial_target: None,
                scope: Some(NetworkPermissionScope {
                    max_redirects: 3,
                    may_cross_origin: true,
                    ssrf_each_hop: true,
                }),
                denial_reason: None,
            },
            NetworkPermissionPreview {
                authorizable: true,
                full_args: json!({ "url": "https://example.com" }),
                canonical_initial_target: Some("https://example.com/".to_string()),
                scope: None,
                denial_reason: None,
            },
            NetworkPermissionPreview {
                authorizable: true,
                full_args: json!({ "url": "https://example.com" }),
                canonical_initial_target: Some("https://example.com/".to_string()),
                scope: Some(NetworkPermissionScope {
                    max_redirects: 3,
                    may_cross_origin: true,
                    ssrf_each_hop: true,
                }),
                denial_reason: Some("unexpected reason".to_string()),
            },
        ] {
            let tool = PreviewTool::new(preview.clone());
            let decider = RecordingDecider::new(PermissionDecision::Allow);

            assert_network_unauthorizable(gate(&network_call(), &tool, &decider).await);
            assert_eq!(decider.calls(), 1);
            assert_eq!(decider.seen_preview(), Some(preview));
            assert_eq!(tool.preview_calls(), 1);
        }
    }

    // --- §1.1 PermissionMode × PermissionLevel 矩阵(卡点 A) ---

    #[test]
    fn auto_allows_normal_mode_never_auto_allows_edit_or_execute() {
        assert!(!auto_allows(
            PermissionMode::Normal,
            PermissionLevel::Network
        ));
        assert!(!auto_allows(PermissionMode::Normal, PermissionLevel::Edit));
        assert!(!auto_allows(
            PermissionMode::Normal,
            PermissionLevel::Execute
        ));
    }

    #[test]
    fn auto_allows_accept_edits_mode_allows_edit_not_execute() {
        assert!(!auto_allows(
            PermissionMode::AcceptEdits,
            PermissionLevel::Network
        ));
        assert!(auto_allows(
            PermissionMode::AcceptEdits,
            PermissionLevel::Edit
        ));
        assert!(!auto_allows(
            PermissionMode::AcceptEdits,
            PermissionLevel::Execute
        ));
    }

    #[test]
    fn auto_allows_yolo_mode_allows_edit_and_execute() {
        assert!(auto_allows(PermissionMode::Yolo, PermissionLevel::Edit));
        assert!(auto_allows(PermissionMode::Yolo, PermissionLevel::Execute));
        assert!(auto_allows(PermissionMode::Yolo, PermissionLevel::Network));
    }

    #[test]
    fn auto_allows_plan_mode_never_auto_allows_edit_or_execute() {
        assert!(!auto_allows(PermissionMode::Plan, PermissionLevel::Network));
        assert!(!auto_allows(PermissionMode::Plan, PermissionLevel::Edit));
        assert!(!auto_allows(PermissionMode::Plan, PermissionLevel::Execute));
        assert!(!auto_allows(
            PermissionMode::Plan,
            PermissionLevel::ReadOnly
        ));
    }

    #[test]
    fn cycle_permission_mode_rotates_normal_accept_edits_yolo_plan() {
        assert_eq!(
            cycle_permission_mode(PermissionMode::Normal),
            PermissionMode::AcceptEdits
        );
        assert_eq!(
            cycle_permission_mode(PermissionMode::AcceptEdits),
            PermissionMode::Yolo
        );
        assert_eq!(
            cycle_permission_mode(PermissionMode::Yolo),
            PermissionMode::Plan
        );
        assert_eq!(
            cycle_permission_mode(PermissionMode::Plan),
            PermissionMode::Normal
        );
    }

    #[test]
    fn permission_mode_label_plan() {
        assert_eq!(permission_mode_label(PermissionMode::Plan), "plan");
    }
}
