use crate::agent::{AgentObserver, AgentStatus};
use crate::permission::{auto_allows, PermissionDecider, PermissionDecision, PermissionMode};
use crate::provider::{DeltaSink, ToolCall};
use crate::tool::{Tool, ToolOutcome};
use async_trait::async_trait;
use serde_json::Value;
use std::sync::{Arc, Mutex};
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
    /// 手动 /compact 完成(成功或失败均发):置回 Ready 并作为排队推进闸门事件。
    CompactDone,
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
    mode: Arc<Mutex<PermissionMode>>,
}

impl ChannelDecider {
    pub fn new(tx: mpsc::UnboundedSender<AgentEvent>, mode: Arc<Mutex<PermissionMode>>) -> Self {
        Self { tx, mode }
    }
}

#[async_trait]
impl PermissionDecider for ChannelDecider {
    async fn decide(&self, call: &ToolCall, tool: &dyn Tool) -> PermissionDecision {
        let mode = *self.mode.lock().expect("permission mode mutex poisoned");
        if auto_allows(mode, tool.permission_level()) {
            return PermissionDecision::Allow;
        }

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
    use crate::permission::{PermissionDecider, PermissionDecision, PermissionMode};
    use crate::provider::{DeltaSink, ToolCall};
    use crate::tool::{PermissionLevel, Tool, ToolContext, ToolOutcome};
    use async_trait::async_trait;
    use serde_json::{json, Value};
    use std::sync::{Arc, Mutex};
    use tokio::sync::mpsc;
    use tokio::time::{sleep, timeout, Duration};

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
        let (tx, rx) = mpsc::unbounded_channel();
        let mode = Arc::new(Mutex::new(mode));
        (ChannelDecider::new(tx, mode.clone()), rx, mode)
    }

    // --- §2.1 ChannelDecider + PermissionMode(卡点 B) ---

    #[tokio::test]
    async fn channel_decider_yolo_auto_allows_execute_without_channel() {
        let (decider, mut rx, _mode) = decider_with_mode(PermissionMode::Yolo);

        let decision = timeout(
            Duration::from_millis(50),
            decider.decide(&execute_call(), &ExecuteTool),
        )
        .await
        .expect("Yolo + Execute must return Allow immediately without channel round-trip");

        assert_eq!(decision, PermissionDecision::Allow);
        assert!(rx.try_recv().is_err(), "must not send PermissionRequired");
    }

    #[tokio::test]
    async fn channel_decider_accept_edits_auto_allows_edit_without_channel() {
        let (decider, mut rx, _mode) = decider_with_mode(PermissionMode::AcceptEdits);

        let decision = timeout(
            Duration::from_millis(50),
            decider.decide(&edit_call(), &EditTool),
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
        let decision = decider.decide(&call, &tool);
        tokio::pin!(decision);

        let request = tokio::select! {
            event = rx.recv() => event.expect("permission request should be sent"),
            decision = &mut decision => panic!("decide returned before permission response: {decision:?}"),
            _ = sleep(Duration::from_millis(50)) => panic!("timed out waiting for permission request"),
        };

        match request {
            AgentEvent::PermissionRequired(request) => {
                assert_eq!(request.tool_name, "execute_tool");
                request.responder.send(PermissionDecision::Allow).unwrap();
            }
            other => panic!("expected PermissionRequired, got {other:?}"),
        }

        assert_eq!(decision.await, PermissionDecision::Allow);
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
        let (decider, mut rx, _mode) = decider_with_mode(PermissionMode::Normal);
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
