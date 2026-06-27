use crate::permission::PermissionDecider;
use crate::permission::PermissionDecision;
use crate::provider::{DeltaSink, ToolCall};
use crate::tool::Tool;
use async_trait::async_trait;
use serde_json::Value;
use tokio::sync::{mpsc, oneshot};

#[derive(Debug)]
pub enum AgentEvent {
    TextDelta(String),
    PermissionRequired(PermissionRequest),
    TurnComplete,
    Error(String),
}

#[derive(Debug, PartialEq, Eq)]
pub enum UserInput {
    Prompt(String),
}

#[derive(Debug)]
pub struct PermissionRequest {
    pub tool_name: String,
    pub args: Value,
    pub responder: oneshot::Sender<PermissionDecision>,
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
}

pub struct ChannelDecider {
    tx: mpsc::UnboundedSender<AgentEvent>,
}

impl ChannelDecider {
    pub fn new(tx: mpsc::UnboundedSender<AgentEvent>) -> Self {
        Self { tx }
    }
}

#[async_trait]
impl PermissionDecider for ChannelDecider {
    async fn decide(&self, call: &ToolCall, tool: &dyn Tool) -> PermissionDecision {
        let (tx, rx) = oneshot::channel();
        let request = PermissionRequest {
            tool_name: tool.name().to_string(),
            args: call.arguments.clone(),
            responder: tx,
        };

        if self
            .tx
            .send(AgentEvent::PermissionRequired(request))
            .is_err()
        {
            return PermissionDecision::Deny;
        }

        rx.await.unwrap_or(PermissionDecision::Deny)
    }
}

#[cfg(test)]
mod tests {
    use super::{AgentEvent, ChannelDecider, ChannelSink};
    use crate::permission::{PermissionDecider, PermissionDecision};
    use crate::provider::{DeltaSink, ToolCall};
    use crate::tool::{PermissionLevel, Tool, ToolContext, ToolOutcome};
    use async_trait::async_trait;
    use serde_json::{json, Value};
    use tokio::sync::mpsc;
    use tokio::time::{sleep, Duration};

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
            PermissionLevel::RequiresConfirmation
        }

        async fn execute(&self, _args: Value, _ctx: &ToolContext) -> ToolOutcome {
            ToolOutcome {
                content: "ok".to_string(),
                is_error: false,
                truncated: false,
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
        let (tx, mut rx) = mpsc::unbounded_channel();
        let decider = ChannelDecider::new(tx);
        let call = call();
        let tool = ConfirmTool;
        let decision = decider.decide(&call, &tool);
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
                request.responder.send(PermissionDecision::Allow).unwrap();
            }
            other => panic!("expected PermissionRequired, got {other:?}"),
        }

        assert_eq!(decision.await, PermissionDecision::Allow);
    }

    #[tokio::test]
    async fn channel_decider_denies_when_permission_responder_is_dropped() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let decider = ChannelDecider::new(tx);
        let call = call();
        let tool = ConfirmTool;
        let decision = decider.decide(&call, &tool);
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
}
