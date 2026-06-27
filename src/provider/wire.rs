use crate::agent::message::Message;
use crate::error::ProviderError;
use crate::provider::{FinishReason, ModelRequest, ModelResponse, ToolCall};
use serde_json::{json, Value};

pub fn serialize_request(req: &ModelRequest) -> Value {
    let messages = req
        .messages
        .iter()
        .map(serialize_message)
        .collect::<Vec<_>>();
    let mut body = json!({
        "model": req.model,
        "messages": messages,
    });

    if let Some(max_tokens) = req.max_tokens {
        body["max_tokens"] = json!(max_tokens);
    }

    if !req.tools.is_empty() {
        body["tools"] = json!(req
            .tools
            .iter()
            .map(|tool| {
                json!({
                    "type": "function",
                    "function": {
                        "name": tool.name,
                        "description": tool.description,
                        "parameters": tool.parameters,
                    }
                })
            })
            .collect::<Vec<_>>());
    }

    body
}

fn serialize_message(message: &Message) -> Value {
    match message {
        Message::System(content) => json!({
            "role": "system",
            "content": content,
        }),
        Message::User(content) => json!({
            "role": "user",
            "content": content,
        }),
        Message::Assistant { text, tool_calls } => {
            let content = if text.is_empty() && !tool_calls.is_empty() {
                Value::Null
            } else {
                json!(text)
            };
            let mut message = json!({
                "role": "assistant",
                "content": content,
            });

            if !tool_calls.is_empty() {
                message["tool_calls"] = json!(tool_calls
                    .iter()
                    .map(|call| {
                        json!({
                            "id": call.id,
                            "type": "function",
                            "function": {
                                "name": call.name,
                                "arguments": serde_json::to_string(&call.arguments)
                                    .expect("serde_json::Value always serializes"),
                            }
                        })
                    })
                    .collect::<Vec<_>>());
            }

            message
        }
        Message::ToolResult {
            call_id, content, ..
        } => json!({
            "role": "tool",
            "tool_call_id": call_id,
            "content": content,
        }),
    }
}

pub fn parse_response(body: &Value) -> Result<ModelResponse, ProviderError> {
    let choice = body
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|choices| choices.first())
        .ok_or_else(|| ProviderError::Decode("missing choices[0]".to_string()))?;
    let message = choice
        .get("message")
        .ok_or_else(|| ProviderError::Decode("missing choices[0].message".to_string()))?;

    let text = match message.get("content") {
        Some(Value::String(content)) => content.clone(),
        Some(Value::Null) | None => String::new(),
        _ => {
            return Err(ProviderError::Decode(
                "message.content must be string or null".to_string(),
            ))
        }
    };
    let tool_calls = parse_tool_calls(message.get("tool_calls"))?;
    let finish_reason = parse_finish_reason(choice.get("finish_reason"));

    Ok(ModelResponse {
        text,
        tool_calls,
        finish_reason,
    })
}

fn parse_tool_calls(value: Option<&Value>) -> Result<Vec<ToolCall>, ProviderError> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    let Value::Array(calls) = value else {
        return Err(ProviderError::Decode(
            "message.tool_calls must be an array".to_string(),
        ));
    };

    calls
        .iter()
        .map(|call| {
            let id = required_string(call, "id")?.to_string();
            let function = call
                .get("function")
                .ok_or_else(|| ProviderError::Decode("tool_call.function missing".to_string()))?;
            let name = required_string(function, "name")?.to_string();
            let arguments = required_string(function, "arguments")?;
            let arguments = serde_json::from_str(arguments).map_err(|err| {
                ProviderError::Decode(format!("tool_call.function.arguments invalid JSON: {err}"))
            })?;

            Ok(ToolCall {
                id,
                name,
                arguments,
            })
        })
        .collect()
}

fn required_string<'a>(value: &'a Value, field: &str) -> Result<&'a str, ProviderError> {
    value
        .get(field)
        .and_then(Value::as_str)
        .ok_or_else(|| ProviderError::Decode(format!("{field} missing or not a string")))
}

fn parse_finish_reason(value: Option<&Value>) -> FinishReason {
    match value.and_then(Value::as_str) {
        Some("stop") => FinishReason::Stop,
        Some("length") => FinishReason::Length,
        Some("tool_calls") => FinishReason::ToolCalls,
        Some(other) => FinishReason::Other(other.to_string()),
        None => FinishReason::Other(String::new()),
    }
}

#[cfg(test)]
mod tests {
    use super::{parse_response, serialize_request};
    use crate::agent::message::Message;
    use crate::error::ProviderError;
    use crate::provider::{FinishReason, ModelRequest, ModelResponse, ToolCall, ToolSchema};
    use serde_json::json;

    #[test]
    fn serialize_request_maps_messages_to_openai_roles_and_fields() {
        let req = ModelRequest {
            model: "gpt-test".to_string(),
            messages: vec![
                Message::System("system prompt".to_string()),
                Message::User("user prompt".to_string()),
                Message::Assistant {
                    text: String::new(),
                    tool_calls: vec![ToolCall {
                        id: "call-1".to_string(),
                        name: "lookup".to_string(),
                        arguments: json!({ "query": "rust" }),
                    }],
                },
                Message::ToolResult {
                    call_id: "call-1".to_string(),
                    content: "tool result".to_string(),
                    is_error: true,
                },
            ],
            tools: Vec::new(),
            max_tokens: Some(128),
        };

        let body = serialize_request(&req);

        assert_eq!(
            body,
            json!({
                "model": "gpt-test",
                "max_tokens": 128,
                "messages": [
                    { "role": "system", "content": "system prompt" },
                    { "role": "user", "content": "user prompt" },
                    {
                        "role": "assistant",
                        "content": null,
                        "tool_calls": [
                            {
                                "id": "call-1",
                                "type": "function",
                                "function": {
                                    "name": "lookup",
                                    "arguments": "{\"query\":\"rust\"}"
                                }
                            }
                        ]
                    },
                    {
                        "role": "tool",
                        "tool_call_id": "call-1",
                        "content": "tool result"
                    }
                ]
            })
        );
    }

    #[test]
    fn serialize_request_includes_tools_only_when_present() {
        let req = ModelRequest {
            model: "gpt-test".to_string(),
            messages: vec![Message::User("use tools".to_string())],
            tools: vec![
                ToolSchema {
                    name: "read_file".to_string(),
                    description: "Read a file".to_string(),
                    parameters: json!({
                        "type": "object",
                        "properties": {
                            "path": { "type": "string" }
                        },
                        "required": ["path"]
                    }),
                },
                ToolSchema {
                    name: "list_dir".to_string(),
                    description: "List a directory".to_string(),
                    parameters: json!({
                        "type": "object",
                        "properties": {
                            "path": { "type": "string" }
                        }
                    }),
                },
            ],
            max_tokens: None,
        };

        let body = serialize_request(&req);

        assert_eq!(
            body["tools"],
            json!([
                {
                    "type": "function",
                    "function": {
                        "name": "read_file",
                        "description": "Read a file",
                        "parameters": {
                            "type": "object",
                            "properties": {
                                "path": { "type": "string" }
                            },
                            "required": ["path"]
                        }
                    }
                },
                {
                    "type": "function",
                    "function": {
                        "name": "list_dir",
                        "description": "List a directory",
                        "parameters": {
                            "type": "object",
                            "properties": {
                                "path": { "type": "string" }
                            }
                        }
                    }
                }
            ])
        );

        let no_tools_req = ModelRequest {
            model: "gpt-test".to_string(),
            messages: vec![Message::User("no tools".to_string())],
            tools: Vec::new(),
            max_tokens: None,
        };

        assert!(serialize_request(&no_tools_req).get("tools").is_none());
    }

    #[test]
    fn parse_response_maps_text_response_to_model_response() {
        let body = json!({
            "choices": [
                {
                    "message": {
                        "role": "assistant",
                        "content": "hello"
                    },
                    "finish_reason": "stop"
                }
            ]
        });

        assert_eq!(
            parse_response(&body).unwrap(),
            ModelResponse {
                text: "hello".to_string(),
                tool_calls: Vec::new(),
                finish_reason: FinishReason::Stop,
            }
        );
    }

    #[test]
    fn parse_response_normalizes_tool_calls_arguments_json() {
        let body = json!({
            "choices": [
                {
                    "message": {
                        "role": "assistant",
                        "content": null,
                        "tool_calls": [
                            {
                                "id": "call-1",
                                "type": "function",
                                "function": {
                                    "name": "lookup",
                                    "arguments": "{\"query\":\"rust\"}"
                                }
                            }
                        ]
                    },
                    "finish_reason": "tool_calls"
                }
            ]
        });

        let parsed = parse_response(&body).unwrap();

        assert_eq!(parsed.text, "");
        assert_eq!(parsed.finish_reason, FinishReason::ToolCalls);
        assert_eq!(
            parsed.tool_calls,
            vec![ToolCall {
                id: "call-1".to_string(),
                name: "lookup".to_string(),
                arguments: json!({ "query": "rust" }),
            }]
        );
    }

    #[test]
    fn parse_response_maps_length_and_unknown_finish_reasons() {
        let length_body = json!({
            "choices": [
                {
                    "message": {
                        "role": "assistant",
                        "content": "partial"
                    },
                    "finish_reason": "length"
                }
            ]
        });
        let unknown_body = json!({
            "choices": [
                {
                    "message": {
                        "role": "assistant",
                        "content": "filtered"
                    },
                    "finish_reason": "content_filter"
                }
            ]
        });

        assert_eq!(
            parse_response(&length_body).unwrap().finish_reason,
            FinishReason::Length
        );
        assert_eq!(
            parse_response(&unknown_body).unwrap().finish_reason,
            FinishReason::Other("content_filter".to_string())
        );
    }

    #[test]
    fn parse_response_returns_decode_error_for_invalid_body() {
        let err = parse_response(&json!({ "choices": [] })).unwrap_err();

        assert!(matches!(err, ProviderError::Decode(_)));
    }
}
