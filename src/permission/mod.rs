use crate::provider::ToolCall;
use crate::tool::{PermissionLevel, Tool};
use async_trait::async_trait;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PermissionDecision {
    Allow,
    Deny,
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
        PermissionLevel::RequiresConfirmation => decider.decide(call, tool).await,
    }
}

#[cfg(test)]
mod tests {
    use super::{gate, PermissionDecider, PermissionDecision};
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
    async fn gate_uses_decider_for_tools_requiring_confirmation() {
        let tool = MockTool {
            permission_level: PermissionLevel::RequiresConfirmation,
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
