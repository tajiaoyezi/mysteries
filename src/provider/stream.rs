use crate::error::ProviderError;
use crate::provider::transport::SseAccumulator;
use crate::provider::{DeltaSink, FinishReason, ModelResponse, ToolCall};
use serde_json::Value;
use std::collections::BTreeMap;

pub struct StreamAccumulator {
    buffer: Vec<u8>,
    text: String,
    tool_calls: BTreeMap<usize, PartialToolCall>,
    finish_reason: FinishReason,
    finished: bool,
}

#[derive(Default)]
struct PartialToolCall {
    id: Option<String>,
    name: Option<String>,
    arguments: String,
}

impl StreamAccumulator {
    pub fn new() -> Self {
        Self {
            buffer: Vec::new(),
            text: String::new(),
            tool_calls: BTreeMap::new(),
            finish_reason: FinishReason::Other(String::new()),
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
        let mut data_lines = Vec::new();

        for line in event.lines().map(|line| line.trim_end_matches('\r')) {
            let line = line.trim_start();
            if line.is_empty() || line.starts_with(':') || line.starts_with("event:") {
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

        self.apply_data(&data, sink)?;
        Ok(None)
    }

    fn apply_data(&mut self, data: &str, sink: &dyn DeltaSink) -> Result<(), ProviderError> {
        let body: Value = serde_json::from_str(data)
            .map_err(|err| ProviderError::Decode(format!("SSE data JSON invalid: {err}")))?;
        let choice = body
            .get("choices")
            .and_then(Value::as_array)
            .and_then(|choices| choices.first())
            .ok_or_else(|| ProviderError::Decode("missing choices[0]".to_string()))?;

        if let Some(reason) = choice.get("finish_reason").and_then(Value::as_str) {
            self.finish_reason = parse_finish_reason(reason);
        }

        let Some(delta) = choice.get("delta") else {
            return Ok(());
        };

        if let Some(content) = delta.get("content").and_then(Value::as_str) {
            self.text.push_str(content);
            sink.on_text(content);
        }

        if let Some(tool_calls) = delta.get("tool_calls").and_then(Value::as_array) {
            for call in tool_calls {
                let index =
                    call.get("index").and_then(Value::as_u64).ok_or_else(|| {
                        ProviderError::Decode("tool_call.index missing".to_string())
                    })? as usize;
                let partial = self.tool_calls.entry(index).or_default();

                if partial.id.is_none() {
                    if let Some(id) = call.get("id").and_then(Value::as_str) {
                        partial.id = Some(id.to_string());
                    }
                }

                let Some(function) = call.get("function") else {
                    continue;
                };

                if partial.name.is_none() {
                    if let Some(name) = function.get("name").and_then(Value::as_str) {
                        partial.name = Some(name.to_string());
                    }
                }

                if let Some(arguments) = function.get("arguments").and_then(Value::as_str) {
                    partial.arguments.push_str(arguments);
                }
            }
        }

        Ok(())
    }

    pub fn finish(&self) -> Result<ModelResponse, ProviderError> {
        let mut tool_calls = Vec::new();

        for partial in self.tool_calls.values() {
            let id = partial
                .id
                .clone()
                .ok_or_else(|| ProviderError::Decode("tool_call.id missing".to_string()))?;
            let name = partial.name.clone().ok_or_else(|| {
                ProviderError::Decode("tool_call.function.name missing".to_string())
            })?;
            let arguments = serde_json::from_str(&partial.arguments).map_err(|err| {
                ProviderError::Decode(format!("tool_call.function.arguments invalid JSON: {err}"))
            })?;

            tool_calls.push(ToolCall {
                id,
                name,
                arguments,
            });
        }

        Ok(ModelResponse {
            text: self.text.clone(),
            tool_calls,
            finish_reason: self.finish_reason.clone(),
        })
    }
}

impl SseAccumulator for StreamAccumulator {
    fn push_chunk(
        &mut self,
        chunk: &[u8],
        sink: &dyn DeltaSink,
    ) -> Result<Option<ModelResponse>, ProviderError> {
        Self::push_chunk(self, chunk, sink)
    }

    fn finish(&self) -> Result<ModelResponse, ProviderError> {
        Self::finish(self)
    }
}

impl Default for StreamAccumulator {
    fn default() -> Self {
        Self::new()
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

fn parse_finish_reason(reason: &str) -> FinishReason {
    match reason {
        "stop" => FinishReason::Stop,
        "length" => FinishReason::Length,
        "tool_calls" => FinishReason::ToolCalls,
        other => FinishReason::Other(other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::StreamAccumulator;
    use crate::error::ProviderError;
    use crate::provider::{DeltaSink, FinishReason, ModelResponse, ToolCall};
    use serde_json::json;
    use std::sync::Mutex;

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

    fn push_all(
        accumulator: &mut StreamAccumulator,
        sink: &CaptureSink,
        chunks: &[&[u8]],
    ) -> Result<ModelResponse, ProviderError> {
        let mut response = None;
        for chunk in chunks {
            if let Some(next) = accumulator.push_chunk(chunk, sink)? {
                response = Some(next);
            }
        }
        response.ok_or_else(|| ProviderError::Decode("stream did not finish".to_string()))
    }

    #[test]
    fn text_deltas_are_pushed_immediately_and_done_returns_response() {
        let mut accumulator = StreamAccumulator::new();
        let sink = CaptureSink::new();

        assert!(accumulator
            .push_chunk(
                br#"data: {"choices":[{"delta":{"content":"Hel"},"finish_reason":null}]}

"#,
                &sink,
            )
            .unwrap()
            .is_none());
        assert_eq!(sink.chunks(), vec!["Hel"]);

        let response = push_all(
            &mut accumulator,
            &sink,
            &[
                br#"data: {"choices":[{"delta":{"content":"lo"},"finish_reason":null}]}

"#,
                br#"data: {"choices":[{"delta":{},"finish_reason":"stop"}]}

data: [DONE]

"#,
            ],
        )
        .unwrap();

        assert_eq!(sink.chunks(), vec!["Hel", "lo"]);
        assert_eq!(response.text, "Hello");
        assert_eq!(response.tool_calls, Vec::<ToolCall>::new());
        assert_eq!(response.finish_reason, FinishReason::Stop);
    }

    #[test]
    fn tool_call_deltas_are_accumulated_by_index() {
        let mut accumulator = StreamAccumulator::new();
        let sink = CaptureSink::new();

        let response = push_all(
            &mut accumulator,
            &sink,
            &[
                br#"data: {"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_1","type":"function","function":{"name":"lookup","arguments":"{\"query\""}}]},"finish_reason":null}]}

"#,
                br#"data: {"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":":\"rust\"}"}}]},"finish_reason":null}]}

"#,
                br#"data: {"choices":[{"delta":{},"finish_reason":"tool_calls"}]}

data: [DONE]

"#,
            ],
        )
        .unwrap();

        assert_eq!(sink.chunks(), Vec::<String>::new());
        assert_eq!(response.text, "");
        assert_eq!(
            response.tool_calls,
            vec![ToolCall {
                id: "call_1".to_string(),
                name: "lookup".to_string(),
                arguments: json!({ "query": "rust" }),
            }]
        );
        assert_eq!(response.finish_reason, FinishReason::ToolCalls);
    }

    #[test]
    fn event_split_across_chunk_boundary_is_stitched() {
        let mut accumulator = StreamAccumulator::new();
        let sink = CaptureSink::new();
        let stream = br#"data: {"choices":[{"delta":{"content":"split"},"finish_reason":null}]}

data: {"choices":[{"delta":{},"finish_reason":"stop"}]}

data: [DONE]

"#;

        let response = push_all(
            &mut accumulator,
            &sink,
            &[&stream[..19], &stream[19..57], &stream[57..]],
        )
        .unwrap();

        assert_eq!(sink.chunks(), vec!["split"]);
        assert_eq!(response.text, "split");
        assert_eq!(response.finish_reason, FinishReason::Stop);
    }

    #[test]
    fn invalid_tool_call_arguments_returns_decode_error() {
        let mut accumulator = StreamAccumulator::new();
        let sink = CaptureSink::new();

        let err = push_all(
            &mut accumulator,
            &sink,
            &[
                br#"data: {"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_1","type":"function","function":{"name":"lookup","arguments":"{\"query\""}}]},"finish_reason":null}]}

"#,
                br#"data: {"choices":[{"delta":{},"finish_reason":"tool_calls"}]}

data: [DONE]

"#,
            ],
        )
        .unwrap_err();

        assert!(matches!(err, ProviderError::Decode(message) if message.contains("arguments")));
    }
}
