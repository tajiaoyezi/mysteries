use crate::provider::ToolCall;
use crate::tool::{PermissionLevel, Tool};
use async_trait::async_trait;
use std::collections::BTreeSet;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PermissionDecision {
    Allow,
    Deny,
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
    async fn decide(&self, call: &ToolCall, tool: &dyn Tool) -> PermissionDecision;
}

pub async fn gate(
    call: &ToolCall,
    tool: &dyn Tool,
    decider: &dyn PermissionDecider,
) -> PermissionDecision {
    match tool.permission_level() {
        PermissionLevel::ReadOnly => PermissionDecision::Allow,
        PermissionLevel::Edit | PermissionLevel::Execute => decider.decide(call, tool).await,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        auto_allows, cycle_permission_mode, gate, normalize, permission_mode_label,
        PermissionDecider, PermissionDecision, PermissionMode, PolicyEngine,
    };
    use crate::provider::ToolCall;
    use crate::tool::{PermissionLevel, Tool, ToolContext, ToolOutcome};
    use async_trait::async_trait;
    use serde_json::{json, Value};
    use std::collections::BTreeSet;
    use std::sync::atomic::{AtomicUsize, Ordering};

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
        async fn decide(&self, call: &ToolCall, tool: &dyn Tool) -> PermissionDecision {
            self.calls.fetch_add(1, Ordering::SeqCst);
            assert_eq!(call.name, "mock_tool");
            assert_eq!(tool.name(), "mock_tool");
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

        assert_eq!(decision, PermissionDecision::Allow);
        assert_eq!(decider.calls(), 0);
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
                PermissionDecision::Allow
            );
            assert_eq!(allow.calls(), 1);
            assert_eq!(gate(&call(), &tool, &deny).await, PermissionDecision::Deny);
            assert_eq!(deny.calls(), 1);
        }
    }

    // --- §1.1 PermissionMode × PermissionLevel 矩阵(卡点 A) ---

    #[test]
    fn auto_allows_normal_mode_never_auto_allows_edit_or_execute() {
        assert!(!auto_allows(PermissionMode::Normal, PermissionLevel::Edit));
        assert!(!auto_allows(
            PermissionMode::Normal,
            PermissionLevel::Execute
        ));
    }

    #[test]
    fn auto_allows_accept_edits_mode_allows_edit_not_execute() {
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
    }

    #[test]
    fn auto_allows_plan_mode_never_auto_allows_edit_or_execute() {
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
