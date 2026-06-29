use crate::provider::ToolCall;
use crate::tool::{PermissionLevel, Tool};
use async_trait::async_trait;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PermissionDecision {
    Allow,
    Deny,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PermissionMode {
    Normal,
    AcceptEdits,
    Yolo,
}

pub fn auto_allows(mode: PermissionMode, level: PermissionLevel) -> bool {
    match (mode, level) {
        (PermissionMode::Normal, PermissionLevel::Edit | PermissionLevel::Execute) => false,
        (PermissionMode::AcceptEdits, PermissionLevel::Edit) => true,
        (PermissionMode::AcceptEdits, PermissionLevel::Execute) => false,
        (PermissionMode::Yolo, PermissionLevel::Edit | PermissionLevel::Execute) => true,
        (_, PermissionLevel::ReadOnly) => false,
    }
}

pub fn cycle_permission_mode(mode: PermissionMode) -> PermissionMode {
    match mode {
        PermissionMode::Normal => PermissionMode::AcceptEdits,
        PermissionMode::AcceptEdits => PermissionMode::Yolo,
        PermissionMode::Yolo => PermissionMode::Normal,
    }
}

pub fn permission_mode_label(mode: PermissionMode) -> &'static str {
    match mode {
        PermissionMode::Normal => "normal",
        PermissionMode::AcceptEdits => "accept-edits",
        PermissionMode::Yolo => "yolo",
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
    use super::{auto_allows, cycle_permission_mode, gate, PermissionDecider, PermissionDecision, PermissionMode};
    use crate::provider::ToolCall;
    use crate::tool::{PermissionLevel, Tool, ToolContext, ToolOutcome};
    use async_trait::async_trait;
    use serde_json::{json, Value};
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
        assert!(!auto_allows(PermissionMode::Normal, PermissionLevel::Execute));
    }

    #[test]
    fn auto_allows_accept_edits_mode_allows_edit_not_execute() {
        assert!(auto_allows(PermissionMode::AcceptEdits, PermissionLevel::Edit));
        assert!(!auto_allows(PermissionMode::AcceptEdits, PermissionLevel::Execute));
    }

    #[test]
    fn auto_allows_yolo_mode_allows_edit_and_execute() {
        assert!(auto_allows(PermissionMode::Yolo, PermissionLevel::Edit));
        assert!(auto_allows(PermissionMode::Yolo, PermissionLevel::Execute));
    }

    #[test]
    fn cycle_permission_mode_rotates_normal_accept_edits_yolo() {
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
            PermissionMode::Normal
        );
    }
}
