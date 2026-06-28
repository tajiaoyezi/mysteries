## ADDED Requirements

### Requirement: OpenAI token 用量解析

`OpenAiProvider` 的请求体 SHALL 附加 `stream_options: { include_usage: true }`(OpenAI 流式默认不回 usage),与既有 `stream: true` 并存。SSE 累积逻辑 MUST 识别流末尾 `choices` 为空、携带 `usage` 的 usage-only chunk,取其 `prompt_tokens` → `input_tokens`、`completion_tokens` → `output_tokens` 填入 `ModelResponse.usage`;该 usage-only chunk MUST NOT 被误当文本 / 工具增量。usage 缺失 / 解析失败 MUST 降级为 `usage = None`,MUST NOT 影响既有 text / tool_calls / finish_reason 的累积。本 requirement 为叠加,既有「SSE 流式累积」「OpenAiProvider 实 HTTP 请求」requirement 的行为不变。

#### Scenario: 含 usage chunk 的流解析出 usage

- **WHEN** 把含末尾 usage-only chunk(`prompt_tokens` / `completion_tokens`)的 OpenAI SSE 字节流喂给累积器
- **THEN** 最终 `ModelResponse.usage` = `Some(Usage{ input_tokens = prompt_tokens, output_tokens = completion_tokens })`,且 text / tool_calls / finish_reason 与无 usage chunk 时一致

#### Scenario: 请求开启 include_usage

- **WHEN** 以一个 `ModelRequest` 构造 OpenAI chat 请求(离线,不发网络)
- **THEN** 请求体含 `stream_options.include_usage = true`,与既有 `stream: true` 并存

#### Scenario: 无 usage chunk 降级为 None

- **WHEN** 流中不含 usage-only chunk(端点不支持 `include_usage`)
- **THEN** `ModelResponse.usage` 为 `None`,其余字段照常累积、`complete` 不失败
