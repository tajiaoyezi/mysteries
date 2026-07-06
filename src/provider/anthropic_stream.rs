use crate::error::ProviderError;
use crate::provider::transport::SseAccumulator;
use crate::provider::{DeltaSink, FinishReason, ModelResponse, ThinkingBlock, ToolCall, Usage};
use serde_json::Value;
use std::collections::BTreeMap;

pub struct AnthropicAccumulator {
    buffer: Vec<u8>,
    text: String,
    tool_calls: BTreeMap<usize, PartialToolUse>,
    thinking_blocks: BTreeMap<usize, PartialThinkingBlock>,
    finish_reason: FinishReason,
    usage_input_tokens: Option<u32>,
    usage_output_tokens: Option<u32>,
    usage_parse_failed: bool,
    finished: bool,
}

#[derive(Default)]
struct PartialToolUse {
    id: Option<String>,
    name: Option<String>,
    input_json: String,
}

#[derive(Default)]
struct PartialThinkingBlock {
    text: String,
    signature: Option<String>,
    redacted: bool,
}

impl AnthropicAccumulator {
    pub fn new() -> Self {
        Self {
            buffer: Vec::new(),
            text: String::new(),
            tool_calls: BTreeMap::new(),
            thinking_blocks: BTreeMap::new(),
            finish_reason: FinishReason::Other(String::new()),
            usage_input_tokens: None,
            usage_output_tokens: None,
            usage_parse_failed: false,
            finished: false,
        }
    }

    pub fn push_chunk(
        &mut self,
        chunk: &[u8],
        sink: &dyn DeltaSink,
    ) -> Result<Option<ModelResponse>, ProviderError> {
        self.buffer.extend_from_slice(chunk);

        while let Some((delimiter_start, delimiter_len)) = find_event_delimiter(&self.buffer) {
            let event = self.buffer[..delimiter_start].to_vec();
            self.buffer.drain(..delimiter_start + delimiter_len);

            if let Some(response) = self.process_event(&event, sink)? {
                return Ok(Some(response));
            }
        }

        Ok(None)
    }

    fn process_event(
        &mut self,
        event: &[u8],
        sink: &dyn DeltaSink,
    ) -> Result<Option<ModelResponse>, ProviderError> {
        let event = std::str::from_utf8(event)
            .map_err(|err| ProviderError::Decode(format!("SSE event is not UTF-8: {err}")))?;
        let mut event_name = None;
        let mut data_lines = Vec::new();

        for line in event.lines().map(|line| line.trim_end_matches('\r')) {
            let line = line.trim_start();
            if line.is_empty() || line.starts_with(':') {
                continue;
            }
            if let Some(next_event) = line.strip_prefix("event:") {
                event_name = Some(next_event.trim().to_string());
                continue;
            }
            if let Some(data) = line.strip_prefix("data:") {
                data_lines.push(data.trim_start());
            }
        }

        if data_lines.is_empty() {
            return Ok(None);
        }

        let data = data_lines.join("\n");
        if data.trim() == "[DONE]" {
            self.finished = true;
            return self.finish().map(Some);
        }

        let body: Value = serde_json::from_str(&data)
            .map_err(|err| ProviderError::Decode(format!("SSE data JSON invalid: {err}")))?;
        let event_type = body
            .get("type")
            .and_then(Value::as_str)
            .or(event_name.as_deref());

        match event_type {
            Some("message_start") => self.apply_message_start(&body),
            Some("content_block_start") => self.apply_content_block_start(&body)?,
            Some("content_block_delta") => self.apply_content_block_delta(&body, sink)?,
            Some("message_delta") => self.apply_message_delta(&body),
            Some("message_stop") => {
                self.finished = true;
                return self.finish().map(Some);
            }
            Some("content_block_stop" | "ping") | None => {}
            Some(_) => {}
        }

        Ok(None)
    }

    fn apply_content_block_start(&mut self, body: &Value) -> Result<(), ProviderError> {
        let index = required_index(body)?;
        let Some(block) = body.get("content_block") else {
            return Ok(());
        };

        if block.get("type").and_then(Value::as_str) == Some("tool_use") {
            let partial = self.tool_calls.entry(index).or_default();
            if partial.id.is_none() {
                if let Some(id) = block.get("id").and_then(Value::as_str) {
                    partial.id = Some(id.to_string());
                }
            }
            if partial.name.is_none() {
                if let Some(name) = block.get("name").and_then(Value::as_str) {
                    partial.name = Some(name.to_string());
                }
            }

            if partial.input_json.is_empty() {
                if let Some(input) = block.get("input") {
                    if input.is_object()
                        && input.as_object().is_some_and(|object| !object.is_empty())
                    {
                        partial.input_json =
                            serde_json::to_string(input).expect("Value serializes");
                    }
                }
            }

            return Ok(());
        }

        if block.get("type").and_then(Value::as_str) == Some("thinking") {
            self.thinking_blocks.entry(index).or_default();
            return Ok(());
        }

        if block.get("type").and_then(Value::as_str) == Some("redacted_thinking") {
            let partial = self.thinking_blocks.entry(index).or_default();
            partial.redacted = true;
            if let Some(data) = block.get("data").and_then(Value::as_str) {
                partial.signature = Some(data.to_string());
            }
        }

        Ok(())
    }

    fn apply_message_start(&mut self, body: &Value) {
        let Some(usage) = body.get("message").and_then(|message| message.get("usage")) else {
            return;
        };

        match optional_u32_field(usage, "input_tokens") {
            Ok(tokens) => self.usage_input_tokens = Some(tokens.unwrap_or(0)),
            Err(()) => self.usage_parse_failed = true,
        }
    }

    fn apply_content_block_delta(
        &mut self,
        body: &Value,
        sink: &dyn DeltaSink,
    ) -> Result<(), ProviderError> {
        let index = required_index(body)?;
        let delta = body.get("delta").ok_or_else(|| {
            ProviderError::Decode("content_block_delta.delta missing".to_string())
        })?;

        match delta.get("type").and_then(Value::as_str) {
            Some("text_delta") => {
                let text = delta
                    .get("text")
                    .and_then(Value::as_str)
                    .ok_or_else(|| ProviderError::Decode("text_delta.text missing".to_string()))?;
                self.text.push_str(text);
                sink.on_text(text);
            }
            Some("input_json_delta") => {
                let partial_json = delta
                    .get("partial_json")
                    .and_then(Value::as_str)
                    .ok_or_else(|| {
                        ProviderError::Decode("input_json_delta.partial_json missing".to_string())
                    })?;
                self.tool_calls
                    .entry(index)
                    .or_default()
                    .input_json
                    .push_str(partial_json);
            }
            Some("thinking_delta") => {
                let text = delta
                    .get("thinking")
                    .and_then(Value::as_str)
                    .ok_or_else(|| {
                        ProviderError::Decode("thinking_delta.thinking missing".to_string())
                    })?;
                let partial = self.thinking_blocks.entry(index).or_default();
                partial.text.push_str(text);
                sink.on_thinking(text);
            }
            Some("signature_delta") => {
                let signature =
                    delta
                        .get("signature")
                        .and_then(Value::as_str)
                        .ok_or_else(|| {
                            ProviderError::Decode("signature_delta.signature missing".to_string())
                        })?;
                let partial = self.thinking_blocks.entry(index).or_default();
                match &mut partial.signature {
                    Some(existing) => existing.push_str(signature),
                    None => partial.signature = Some(signature.to_string()),
                }
            }
            Some(_) | None => {}
        }

        Ok(())
    }

    fn apply_message_delta(&mut self, body: &Value) {
        if let Some(reason) = body
            .get("delta")
            .and_then(|delta| delta.get("stop_reason"))
            .and_then(Value::as_str)
        {
            self.finish_reason = parse_finish_reason(reason);
        }

        let Some(usage) = body.get("usage") else {
            return;
        };

        match optional_u32_field(usage, "output_tokens") {
            Ok(tokens) => self.usage_output_tokens = Some(tokens.unwrap_or(0)),
            Err(()) => self.usage_parse_failed = true,
        }
    }

    pub fn finish(&self) -> Result<ModelResponse, ProviderError> {
        let mut tool_calls = Vec::new();

        for partial in self.tool_calls.values() {
            let id = partial
                .id
                .clone()
                .ok_or_else(|| ProviderError::Decode("tool_use.id missing".to_string()))?;
            let name = partial
                .name
                .clone()
                .ok_or_else(|| ProviderError::Decode("tool_use.name missing".to_string()))?;
            let input_json = if partial.input_json.trim().is_empty() {
                "{}"
            } else {
                partial.input_json.trim()
            };
            let arguments = serde_json::from_str(input_json).map_err(|err| {
                ProviderError::Decode(format!("tool_use input JSON invalid: {err}"))
            })?;

            tool_calls.push(ToolCall {
                id,
                name,
                arguments,
            });
        }

        let mut thinking = Vec::new();
        for partial in self.thinking_blocks.values() {
            thinking.push(ThinkingBlock {
                text: partial.text.clone(),
                signature: partial.signature.clone(),
                redacted: partial.redacted,
            });
        }

        Ok(ModelResponse {
            text: self.text.clone(),
            tool_calls,
            finish_reason: self.finish_reason.clone(),
            usage: self.usage(),
            thinking,
        })
    }

    fn usage(&self) -> Option<Usage> {
        if self.usage_parse_failed {
            return None;
        }
        if self.usage_input_tokens.is_none() && self.usage_output_tokens.is_none() {
            return None;
        }

        Some(Usage {
            input_tokens: self.usage_input_tokens.unwrap_or(0),
            output_tokens: self.usage_output_tokens.unwrap_or(0),
        })
    }
}

impl SseAccumulator for AnthropicAccumulator {
    fn push_chunk(
        &mut self,
        _chunk: &[u8],
        _sink: &dyn DeltaSink,
    ) -> Result<Option<ModelResponse>, ProviderError> {
        Self::push_chunk(self, _chunk, _sink)
    }

    fn finish(&self) -> Result<ModelResponse, ProviderError> {
        Self::finish(self)
    }
}

impl Default for AnthropicAccumulator {
    fn default() -> Self {
        Self::new()
    }
}

fn required_index(body: &Value) -> Result<usize, ProviderError> {
    body.get("index")
        .and_then(Value::as_u64)
        .map(|index| index as usize)
        .ok_or_else(|| ProviderError::Decode("content_block.index missing".to_string()))
}

fn optional_u32_field(value: &Value, field: &str) -> Result<Option<u32>, ()> {
    let Some(value) = value.get(field) else {
        return Ok(None);
    };
    value
        .as_u64()
        .and_then(|tokens| u32::try_from(tokens).ok())
        .map(Some)
        .ok_or(())
}

fn parse_finish_reason(reason: &str) -> FinishReason {
    match reason {
        "end_turn" => FinishReason::Stop,
        "max_tokens" => FinishReason::Length,
        "tool_use" => FinishReason::ToolCalls,
        other => FinishReason::Other(other.to_string()),
    }
}

fn find_event_delimiter(buffer: &[u8]) -> Option<(usize, usize)> {
    let lf = find_bytes(buffer, b"\n\n").map(|start| (start, 2));
    let crlf = find_bytes(buffer, b"\r\n\r\n").map(|start| (start, 4));

    match (lf, crlf) {
        (Some(lf), Some(crlf)) => Some(if lf.0 <= crlf.0 { lf } else { crlf }),
        (Some(lf), None) => Some(lf),
        (None, Some(crlf)) => Some(crlf),
        (None, None) => None,
    }
}

fn find_bytes(buffer: &[u8], needle: &[u8]) -> Option<usize> {
    buffer
        .windows(needle.len())
        .position(|window| window == needle)
}

#[cfg(test)]
mod tests {
    use super::AnthropicAccumulator;
    use crate::error::ProviderError;
    use crate::provider::transport::accumulate_stream;
    use crate::provider::{DeltaSink, FinishReason, ThinkingBlock, ToolCall, Usage};
    use serde_json::json;
    use std::sync::Mutex;

    const PROVIDER_LABEL: &str = "Anthropic";

    struct CaptureSink {
        chunks: Mutex<Vec<String>>,
    }

    impl CaptureSink {
        fn new() -> Self {
            Self {
                chunks: Mutex::new(Vec::new()),
            }
        }

        fn chunks(&self) -> Vec<String> {
            self.chunks.lock().unwrap().clone()
        }
    }

    impl DeltaSink for CaptureSink {
        fn on_text(&self, text: &str) {
            self.chunks.lock().unwrap().push(text.to_string());
        }
    }

    fn official_tool_use_fixture() -> &'static [u8] {
        br#"event: message_start
data: {"type":"message_start","message":{"id":"msg_014p7gG3wDgGV9EUtLvnow3U","type":"message","role":"assistant","model":"claude-opus-4-8","stop_sequence":null,"usage":{"input_tokens":472,"output_tokens":2},"content":[],"stop_reason":null}}

event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Okay"}}

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":", let's check"}}

event: content_block_stop
data: {"type":"content_block_stop","index":0}

event: content_block_start
data: {"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"toolu_01T1x1fJ34qAmk2tNTrN7Up6","name":"get_weather","input":{}}}

event: content_block_delta
data: {"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":""}}

event: content_block_delta
data: {"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"{\"location\":"}}

event: content_block_delta
data: {"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":" \"San"}}

event: content_block_delta
data: {"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":" Francisco, CA\"}"}}

event: content_block_stop
data: {"type":"content_block_stop","index":1}

event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"tool_use","stop_sequence":null},"usage":{"output_tokens":89}}

event: message_stop
data: {"type":"message_stop"}

"#
    }

    #[tokio::test]
    async fn anthropic_sse_text_and_tool_use_normalize_to_model_response() {
        let sink = CaptureSink::new();
        let stream =
            futures_util::stream::iter([Ok::<_, &'static str>(official_tool_use_fixture())]);

        let response =
            accumulate_stream(stream, &sink, AnthropicAccumulator::new(), PROVIDER_LABEL)
                .await
                .unwrap();

        assert_eq!(sink.chunks(), vec!["Okay", ", let's check"]);
        assert_eq!(response.text, "Okay, let's check");
        assert_eq!(
            response.tool_calls,
            vec![ToolCall {
                id: "toolu_01T1x1fJ34qAmk2tNTrN7Up6".to_string(),
                name: "get_weather".to_string(),
                arguments: json!({ "location": "San Francisco, CA" }),
            }]
        );
        assert_eq!(response.finish_reason, FinishReason::ToolCalls);
        assert_eq!(
            response.usage,
            Some(Usage {
                input_tokens: 472,
                output_tokens: 89,
            })
        );
    }

    #[tokio::test]
    async fn anthropic_sse_without_usage_returns_none() {
        let sink = CaptureSink::new();
        let stream = futures_util::stream::iter([Ok::<_, &'static str>(
            br#"event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hello"}}

event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null}}

event: message_stop
data: {"type":"message_stop"}

"#,
        )]);

        let response =
            accumulate_stream(stream, &sink, AnthropicAccumulator::new(), PROVIDER_LABEL)
                .await
                .unwrap();

        assert_eq!(response.text, "Hello");
        assert_eq!(response.finish_reason, FinishReason::Stop);
        assert_eq!(response.usage, None);
    }

    #[tokio::test]
    async fn anthropic_sse_message_start_usage_without_delta_uses_zero_output_tokens() {
        let sink = CaptureSink::new();
        let stream = futures_util::stream::iter([Ok::<_, &'static str>(
            br#"event: message_start
data: {"type":"message_start","message":{"id":"msg_1","type":"message","role":"assistant","usage":{"input_tokens":33},"content":[],"stop_reason":null}}

event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hello"}}

event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null}}

event: message_stop
data: {"type":"message_stop"}

"#,
        )]);

        let response =
            accumulate_stream(stream, &sink, AnthropicAccumulator::new(), PROVIDER_LABEL)
                .await
                .unwrap();

        assert_eq!(response.text, "Hello");
        assert_eq!(response.finish_reason, FinishReason::Stop);
        assert_eq!(
            response.usage,
            Some(Usage {
                input_tokens: 33,
                output_tokens: 0,
            })
        );
    }

    #[tokio::test]
    async fn anthropic_sse_stitches_events_across_chunk_boundaries() {
        let sink = CaptureSink::new();
        let fixture = official_tool_use_fixture();
        let stream = futures_util::stream::iter([
            Ok::<_, &'static str>(&fixture[..37]),
            Ok::<_, &'static str>(&fixture[37..521]),
            Ok::<_, &'static str>(&fixture[521..]),
        ]);

        let response =
            accumulate_stream(stream, &sink, AnthropicAccumulator::new(), PROVIDER_LABEL)
                .await
                .unwrap();

        assert_eq!(sink.chunks(), vec!["Okay", ", let's check"]);
        assert_eq!(response.text, "Okay, let's check");
        assert_eq!(response.finish_reason, FinishReason::ToolCalls);
    }

    #[tokio::test]
    async fn anthropic_sse_invalid_tool_input_returns_decode() {
        let sink = CaptureSink::new();
        let stream = futures_util::stream::iter([Ok::<_, &'static str>(
            br#"event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"toolu_bad","name":"lookup","input":{}}}

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"{\"location\""}}

event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"tool_use","stop_sequence":null}}

event: message_stop
data: {"type":"message_stop"}

"#,
        )]);

        let err = accumulate_stream(stream, &sink, AnthropicAccumulator::new(), PROVIDER_LABEL)
            .await
            .unwrap_err();

        assert!(matches!(err, ProviderError::Decode(message) if message.contains("tool_use")));
        assert_eq!(sink.chunks(), Vec::<String>::new());
    }

    struct ThinkingCaptureSink {
        text: Mutex<Vec<String>>,
        thinking: Mutex<Vec<String>>,
    }

    impl ThinkingCaptureSink {
        fn new() -> Self {
            Self {
                text: Mutex::new(Vec::new()),
                thinking: Mutex::new(Vec::new()),
            }
        }
    }

    impl DeltaSink for ThinkingCaptureSink {
        fn on_text(&self, text: &str) {
            self.text.lock().unwrap().push(text.to_string());
        }

        fn on_thinking(&self, text: &str) {
            self.thinking.lock().unwrap().push(text.to_string());
        }
    }

    #[tokio::test]
    async fn anthropic_sse_thinking_and_signature_accumulate_into_model_response() {
        let sink = ThinkingCaptureSink::new();
        let stream = futures_util::stream::iter([Ok::<_, &'static str>(
            br#"event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":""}}

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":"Let me "}}

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":"think"}}

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"signature_delta","signature":"sig-1"}}

event: content_block_stop
data: {"type":"content_block_stop","index":0}

event: content_block_start
data: {"type":"content_block_start","index":1,"content_block":{"type":"text","text":""}}

event: content_block_delta
data: {"type":"content_block_delta","index":1,"delta":{"type":"text_delta","text":"done"}}

event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null}}

event: message_stop
data: {"type":"message_stop"}

"#,
        )]);

        let response =
            accumulate_stream(stream, &sink, AnthropicAccumulator::new(), PROVIDER_LABEL)
                .await
                .unwrap();

        assert_eq!(*sink.thinking.lock().unwrap(), vec!["Let me ", "think"]);
        assert_eq!(response.text, "done");
        assert_eq!(
            response.thinking,
            vec![ThinkingBlock {
                text: "Let me think".to_string(),
                signature: Some("sig-1".to_string()),
                redacted: false,
            }]
        );
    }

    #[tokio::test]
    async fn anthropic_sse_redacted_thinking_block_is_marked_redacted() {
        let sink = CaptureSink::new();
        let stream = futures_util::stream::iter([Ok::<_, &'static str>(
            br#"event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"redacted_thinking","data":"secret"}}

event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null}}

event: message_stop
data: {"type":"message_stop"}

"#,
        )]);

        let response =
            accumulate_stream(stream, &sink, AnthropicAccumulator::new(), PROVIDER_LABEL)
                .await
                .unwrap();

        assert_eq!(
            response.thinking,
            vec![ThinkingBlock {
                text: String::new(),
                signature: Some("secret".to_string()),
                redacted: true,
            }]
        );
    }
}
