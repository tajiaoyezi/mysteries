use crate::agent::{AgentObserver, AgentStatus};
use crate::permission::PermissionDecider;
use crate::permission::PermissionDecision;
use crate::provider::{DeltaSink, ToolCall};
use crate::tool::{Tool, ToolOutcome};
use async_trait::async_trait;
use serde_json::Value;
use tokio::sync::{mpsc, oneshot};

#[derive(Debug)]
pub enum AgentEvent {
    TextDelta(String),
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
    Interrupted,
    TurnComplete,
    Notice(String),
    Usage {
        input_tokens: u32,
        output_tokens: u32,
    },
    Error(String),
}

#[derive(Debug, PartialEq, Eq)]
pub enum UserInput {
    Prompt(String),
    SetModel(String),
    SetProvider { id: String, model: String },
    Compact,
    Interrupt,
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

    fn on_tool_call_started(&self, id: &str, name: &str, args: &Value, readonly: bool) {
        let _ = self.tx.send(AgentEvent::ToolCallStarted {
            id: id.to_string(),
            name: name.to_string(),
            args: args.clone(),
            readonly,
        });
    }

    fn on_tool_call_finished(&self, id: &str, outcome: &ToolOutcome) {
        let _ = self.tx.send(AgentEvent::ToolCallFinished {
            id: id.to_string(),
            outcome: outcome.clone(),
        });
    }

    fn on_usage(&self, usage: &crate::provider::Usage) {
        let _ = self.tx.send(AgentEvent::Usage {
            input_tokens: usage.input_tokens,
            output_tokens: usage.output_tokens,
        });
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
    use super::{AgentEvent, ChannelDecider, ChannelObserver, ChannelSink};
    use crate::agent::{AgentObserver, AgentStatus};
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
