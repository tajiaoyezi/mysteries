use crate::agent::message::Message;
use crate::provider::model_meta::{anthropic_thinking_capability, AnthropicThinking};
use crate::provider::{Depth, ModelRequest, ThinkingBlock};
use serde_json::{json, Value};

pub const DEFAULT_MAX_TOKENS: u32 = 1024;

fn budget_ratio(depth: Depth) -> f64 {
    match depth {
        Depth::Off => 0.0,
        Depth::Low => 0.2,
        Depth::Medium => 0.5,
        Depth::High => 0.8,
        Depth::Xhigh => 0.9,
    }
}

fn clamp_budget_tokens(max_tokens: u32, depth: Depth) -> Option<u32> {
    if max_tokens < 1025 {
        return None;
    }
    let min = 1024_u32;
    let max = max_tokens.saturating_sub(1);
    if min > max {
        return None;
    }
    let target = (max_tokens as f64 * budget_ratio(depth)) as u32;
    Some(target.clamp(min, max))
}

pub fn anthropic_thinking_body(
    cap: AnthropicThinking,
    depth: Depth,
    max_tokens: Option<u32>,
) -> (Option<Value>, Option<Value>) {
    match cap {
        AnthropicThinking::None => (None, None),
        AnthropicThinking::Adaptive {
            can_disable,
            max_effort,
        } => {
            if depth == Depth::Off {
                if can_disable {
                    (Some(json!({ "type": "disabled" })), None)
                } else {
                    (None, Some(json!({ "effort": "low" })))
                }
            } else {
                (
                    Some(json!({
                        "type": "adaptive",
                        "display": "summarized",
                    })),
                    Some(json!({ "effort": depth.as_effort(max_effort) })),
                )
            }
        }
        AnthropicThinking::Budget { effort: _ } => {
            if depth == Depth::Off {
                return (None, None);
            }
            let Some(max_tokens) = max_tokens else {
                return (None, None);
            };
            let Some(budget_tokens) = clamp_budget_tokens(max_tokens, depth) else {
                return (None, None);
            };
            (
                Some(json!({
                    "type": "enabled",
                    "budget_tokens": budget_tokens,
                    "display": "summarized",
                })),
                None,
            )
        }
    }
}

fn serialize_thinking_block(block: &ThinkingBlock) -> Value {
    if block.redacted {
        json!({
            "type": "redacted_thinking",
            "data": block.signature.as_deref().unwrap_or(""),
        })
    } else {
        let mut value = json!({
            "type": "thinking",
            "thinking": block.text,
        });
        if let Some(signature) = &block.signature {
            value["signature"] = json!(signature);
        }
        value
    }
}

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
            Message::Assistant {
                text,
                tool_calls,
                thinking,
            } => {
                let mut content = Vec::new();
                for block in thinking {
                    content.push(serialize_thinking_block(block));
                }
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

    if let Some(config) = &req.thinking {
        let cap = anthropic_thinking_capability(&req.model);
        let (thinking, output_config) =
            anthropic_thinking_body(cap, config.depth, req.max_tokens);
        if let Some(thinking) = thinking {
            body["thinking"] = thinking;
        }
        if let Some(output_config) = output_config {
            body["output_config"] = output_config;
        }
    }

    body
}

#[cfg(test)]
mod tests {
    use super::{
        anthropic_thinking_body, serialize_request, AnthropicThinking, DEFAULT_MAX_TOKENS,
    };
    use crate::agent::message::Message;
    use crate::provider::model_meta::anthropic_thinking_capability;
    use crate::provider::{Depth, ModelRequest, ThinkingBlock, ThinkingConfig, ToolCall, ToolSchema};
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
                    thinking: Vec::new(),
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
            thinking: None,
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
            thinking: None,
        };

        let body = serialize_request(&req);

        assert_eq!(body["max_tokens"], json!(64));
    }

    #[test]
    fn adaptive_model_medium_emits_adaptive_and_output_config() {
        let cap = anthropic_thinking_capability("claude-opus-4-8");
        let (thinking, output_config) =
            anthropic_thinking_body(cap, Depth::Medium, Some(16_000));

        assert_eq!(
            thinking,
            Some(json!({
                "type": "adaptive",
                "display": "summarized",
            }))
        );
        assert_eq!(output_config, Some(json!({ "effort": "medium" })));
    }

    #[test]
    fn budget_model_high_emits_enabled_budget_tokens() {
        let cap = anthropic_thinking_capability("claude-haiku-4-5");
        let (thinking, output_config) =
            anthropic_thinking_body(cap, Depth::High, Some(16_000));

        assert_eq!(output_config, None);
        let thinking = thinking.expect("budget branch should emit thinking");
        assert_eq!(thinking["type"], "enabled");
        assert_eq!(thinking["display"], "summarized");
        let budget = thinking["budget_tokens"].as_u64().expect("budget_tokens");
        assert!((1024..16_000).contains(&(budget as u32)));
    }

    #[test]
    fn off_can_disable_emits_disabled() {
        let cap = anthropic_thinking_capability("claude-sonnet-5");
        let (thinking, output_config) =
            anthropic_thinking_body(cap, Depth::Off, Some(16_000));

        assert_eq!(thinking, Some(json!({ "type": "disabled" })));
        assert_eq!(output_config, None);
    }

    #[test]
    fn off_always_on_emits_low_effort_without_thinking() {
        let cap = anthropic_thinking_capability("claude-fable-5");
        let (thinking, output_config) =
            anthropic_thinking_body(cap, Depth::Off, Some(16_000));

        assert_eq!(thinking, None);
        assert_eq!(output_config, Some(json!({ "effort": "low" })));
    }

    #[test]
    fn budget_guard_omits_thinking_when_max_tokens_too_small_or_none() {
        let cap = AnthropicThinking::Budget { effort: false };

        let (thinking_small, _) =
            anthropic_thinking_body(cap.clone(), Depth::High, Some(1_000));
        assert_eq!(thinking_small, None);

        let (thinking_none, _) = anthropic_thinking_body(cap, Depth::High, None);
        assert_eq!(thinking_none, None);
    }

    #[test]
    fn assistant_thinking_blocks_precede_text_and_tool_use() {
        let req = ModelRequest {
            model: "claude-opus-4-8".to_string(),
            messages: vec![Message::Assistant {
                text: "answer".to_string(),
                tool_calls: vec![ToolCall {
                    id: "toolu_01".to_string(),
                    name: "lookup".to_string(),
                    arguments: json!({ "query": "rust" }),
                }],
                thinking: vec![
                    ThinkingBlock {
                        text: "plan".to_string(),
                        signature: Some("sig-abc".to_string()),
                        redacted: false,
                    },
                    ThinkingBlock {
                        text: String::new(),
                        signature: Some("redacted-data".to_string()),
                        redacted: true,
                    },
                ],
            }],
            tools: Vec::new(),
            max_tokens: None,
            thinking: None,
        };

        let content = &serialize_request(&req)["messages"][0]["content"];
        assert_eq!(
            content,
            &json!([
                {
                    "type": "thinking",
                    "thinking": "plan",
                    "signature": "sig-abc",
                },
                {
                    "type": "redacted_thinking",
                    "data": "redacted-data",
                },
                { "type": "text", "text": "answer" },
                {
                    "type": "tool_use",
                    "id": "toolu_01",
                    "name": "lookup",
                    "input": { "query": "rust" },
                }
            ])
        );
    }

    #[test]
    fn serialize_request_wires_adaptive_thinking_from_model_request() {
        let req = ModelRequest {
            model: "claude-opus-4-8".to_string(),
            messages: vec![Message::User("hello".to_string())],
            tools: Vec::new(),
            max_tokens: Some(16_000),
            thinking: Some(ThinkingConfig {
                depth: Depth::Medium,
            }),
        };

        let body = serialize_request(&req);

        assert_eq!(
            body["thinking"],
            json!({
                "type": "adaptive",
                "display": "summarized",
            })
        );
        assert_eq!(body["output_config"], json!({ "effort": "medium" }));
    }

    #[test]
    #[ignore = "manual: cargo test dump_thinking_wire_samples --lib -- --ignored --nocapture"]
    fn dump_thinking_wire_samples() {
        let dump = |name: &str, body: &serde_json::Value| {
            eprintln!(
                "=== {} ===\n{}",
                name,
                serde_json::to_string_pretty(body).unwrap()
            );
        };

        let adaptive = serialize_request(&ModelRequest {
            model: "claude-opus-4-8".to_string(),
            messages: vec![Message::User("hi".to_string())],
            tools: Vec::new(),
            max_tokens: Some(16_000),
            thinking: Some(ThinkingConfig {
                depth: Depth::Medium,
            }),
        });
        dump("anthropic_adaptive_opus48_medium", &adaptive);

        let budget = serialize_request(&ModelRequest {
            model: "claude-haiku-4-5".to_string(),
            messages: vec![Message::User("hi".to_string())],
            tools: Vec::new(),
            max_tokens: Some(16_000),
            thinking: Some(ThinkingConfig {
                depth: Depth::High,
            }),
        });
        dump("anthropic_budget_haiku45_high_16k", &budget);

        let off_disable = serialize_request(&ModelRequest {
            model: "claude-sonnet-5".to_string(),
            messages: vec![Message::User("hi".to_string())],
            tools: Vec::new(),
            max_tokens: Some(16_000),
            thinking: Some(ThinkingConfig {
                depth: Depth::Off,
            }),
        });
        dump("anthropic_off_can_disable_sonnet5", &off_disable);

        let off_always = serialize_request(&ModelRequest {
            model: "claude-fable-5".to_string(),
            messages: vec![Message::User("hi".to_string())],
            tools: Vec::new(),
            max_tokens: Some(16_000),
            thinking: Some(ThinkingConfig {
                depth: Depth::Off,
            }),
        });
        dump("anthropic_off_always_on_fable5", &off_always);

        let guard_small = serialize_request(&ModelRequest {
            model: "claude-haiku-4-5".to_string(),
            messages: vec![Message::User("hi".to_string())],
            tools: Vec::new(),
            max_tokens: Some(1_000),
            thinking: Some(ThinkingConfig {
                depth: Depth::High,
            }),
        });
        dump("anthropic_budget_guard_haiku45_1000", &guard_small);

        let guard_none = serialize_request(&ModelRequest {
            model: "claude-haiku-4-5".to_string(),
            messages: vec![Message::User("hi".to_string())],
            tools: Vec::new(),
            max_tokens: None,
            thinking: Some(ThinkingConfig {
                depth: Depth::High,
            }),
        });
        dump("anthropic_budget_guard_haiku45_none", &guard_none);

        let assistant = serialize_request(&ModelRequest {
            model: "claude-opus-4-8".to_string(),
            messages: vec![Message::Assistant {
                text: "answer".to_string(),
                tool_calls: vec![ToolCall {
                    id: "toolu_01".to_string(),
                    name: "lookup".to_string(),
                    arguments: json!({ "query": "rust" }),
                }],
                thinking: vec![ThinkingBlock {
                    text: "plan".to_string(),
                    signature: Some("sig-abc".to_string()),
                    redacted: false,
                }],
            }],
            tools: Vec::new(),
            max_tokens: None,
            thinking: None,
        });
        eprintln!(
            "=== anthropic_assistant_thinking_tool_use_content ===\n{}",
            serde_json::to_string_pretty(&assistant["messages"][0]["content"]).unwrap()
        );
    }
}
