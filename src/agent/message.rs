use crate::provider::ToolCall;
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
pub enum Message {
    System(String),
    User(String),
    Assistant {
        text: String,
        tool_calls: Vec<ToolCall>,
    },
    ToolResult {
        call_id: String,
        content: String,
        is_error: bool,
    },
}
