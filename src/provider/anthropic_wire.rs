use crate::agent::message::Message;
use crate::provider::ModelRequest;
use serde_json::{json, Value};

pub const DEFAULT_MAX_TOKENS: u32 = 1024;

pub fn serialize_request(req: &ModelRequest) -> Value {
    let mut system_messages = Vec::new();
    let mut messages = Vec::new();

    for message in &req.messages {
        match message {
            Message::System(content) => system_messages.push(content.as_str()),
            Message::User(content) => messages.push(json!({
                "role": "user",
                "content": [
                    { "type": "text", "text": content }
                ],
            })),
            Message::Assistant { text, tool_calls } => {
                let mut content = Vec::new();
                if !text.is_empty() {
                    content.push(json!({ "type": "text", "text": text }));
                }
                for call in tool_calls {
                    content.push(json!({
                        "type": "tool_use",
                        "id": call.id,
                        "name": call.name,
                        "input": call.arguments,
                    }));
                }
                messages.push(json!({
                    "role": "assistant",
                    "content": content,
                }));
            }
            Message::ToolResult {
                call_id,
                content,
                is_error,
            } => messages.push(json!({
                "role": "user",
                "content": [
                    {
                        "type": "tool_result",
                        "tool_use_id": call_id,
                        "content": content,
                        "is_error": is_error,
                    }
                ],
            })),
        }
    }

    let mut body = json!({
        "model": req.model,
        "max_tokens": req.max_tokens.unwrap_or(DEFAULT_MAX_TOKENS),
        "messages": messages,
    });

    if !system_messages.is_empty() {
        body["system"] = json!(system_messages.join("\n\n"));
    }

    if !req.tools.is_empty() {
        body["tools"] = json!(req
            .tools
            .iter()
            .map(|tool| {
                json!({
                    "name": tool.name,
                    "description": tool.description,
                    "input_schema": tool.parameters,
                })
            })
            .collect::<Vec<_>>());
    }

    body
}

#[cfg(test)]
mod tests {
    use super::{serialize_request, DEFAULT_MAX_TOKENS};
    use crate::agent::message::Message;
    use crate::provider::{ModelRequest, ToolCall, ToolSchema};
    use serde_json::json;

    #[test]
    fn serialize_request_maps_anthropic_messages_tools_and_default_max_tokens() {
        let req = ModelRequest {
            model: "claude-test".to_string(),
            messages: vec![
                Message::System("system prompt".to_string()),
                Message::User("user prompt".to_string()),
                Message::Assistant {
                    text: "assistant text".to_string(),
                    tool_calls: vec![ToolCall {
                        id: "toolu_01".to_string(),
                        name: "lookup".to_string(),
                        arguments: json!({ "query": "rust" }),
                    }],
                },
                Message::ToolResult {
                    call_id: "toolu_01".to_string(),
                    content: "tool result".to_string(),
                    is_error: true,
                },
            ],
            tools: vec![ToolSchema {
                name: "lookup".to_string(),
                description: "Lookup data".to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "query": { "type": "string" }
                    },
                    "required": ["query"]
                }),
            }],
            max_tokens: None,
        };

        let body = serialize_request(&req);

        assert_eq!(
            body,
            json!({
                "model": "claude-test",
                "system": "system prompt",
                "max_tokens": DEFAULT_MAX_TOKENS,
                "messages": [
                    {
                        "role": "user",
                        "content": [
                            { "type": "text", "text": "user prompt" }
                        ]
                    },
                    {
                        "role": "assistant",
                        "content": [
                            { "type": "text", "text": "assistant text" },
                            {
                                "type": "tool_use",
                                "id": "toolu_01",
                                "name": "lookup",
                                "input": { "query": "rust" }
                            }
                        ]
                    },
                    {
                        "role": "user",
                        "content": [
                            {
                                "type": "tool_result",
                                "tool_use_id": "toolu_01",
                                "content": "tool result",
                                "is_error": true
                            }
                        ]
                    }
                ],
                "tools": [
                    {
                        "name": "lookup",
                        "description": "Lookup data",
                        "input_schema": {
                            "type": "object",
                            "properties": {
                                "query": { "type": "string" }
                            },
                            "required": ["query"]
                        }
                    }
                ]
            })
        );
    }

    #[test]
    fn serialize_request_uses_explicit_max_tokens_when_present() {
        let req = ModelRequest {
            model: "claude-test".to_string(),
            messages: vec![Message::User("hello".to_string())],
            tools: Vec::new(),
            max_tokens: Some(64),
        };

        let body = serialize_request(&req);

        assert_eq!(body["max_tokens"], json!(64));
    }
}
