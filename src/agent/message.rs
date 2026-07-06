use crate::provider::{ThinkingBlock, ToolCall};
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
pub enum Message {
    System(String),
    User(String),
    Assistant {
        text: String,
        tool_calls: Vec<ToolCall>,
        #[serde(default)]
        thinking: Vec<ThinkingBlock>,
    },
    ToolResult {
        call_id: String,
        content: String,
        is_error: bool,
    },
}

#[cfg(test)]
mod tests {
    use super::Message;
    use crate::provider::ThinkingBlock;

    #[test]
    fn assistant_serde_roundtrip_with_thinking() {
        let msg = Message::Assistant {
            text: "done".to_string(),
            tool_calls: Vec::new(),
            thinking: vec![ThinkingBlock {
                text: "hmm".to_string(),
                signature: Some("sig".to_string()),
                redacted: false,
            }],
        };
        let json = serde_json::to_string(&msg).unwrap();
        let parsed: Message = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, msg);
    }

    #[test]
    fn assistant_serde_roundtrip_without_thinking_key() {
        let msg = Message::Assistant {
            text: "done".to_string(),
            tool_calls: Vec::new(),
            thinking: Vec::new(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        let parsed: Message = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, msg);
    }

    #[test]
    fn assistant_deserializes_legacy_jsonl_without_thinking_key() {
        let legacy = r#"{"Assistant":{"text":"done","tool_calls":[]}}"#;
        let parsed: Message = serde_json::from_str(legacy).unwrap();
        assert_eq!(
            parsed,
            Message::Assistant {
                text: "done".to_string(),
                tool_calls: Vec::new(),
                thinking: Vec::new(),
            }
        );
    }
}
