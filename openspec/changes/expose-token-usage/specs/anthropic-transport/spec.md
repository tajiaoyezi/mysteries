## ADDED Requirements

### Requirement: Anthropic token 用量解析

Anthropic SSE 累积逻辑 SHALL 从 `message_start` 事件的 `message.usage.input_tokens` 与 `message_delta`(`message_stop` 前最后一次)的 `usage.output_tokens` 收集 token 用量,合成 `ModelResponse.usage = Some(Usage{ input_tokens, output_tokens })`,归一化为与 OpenAI **同一** `Usage` 形状;任一字段缺失记 `0`,两类事件均无 usage 才为 `None`。usage 解析 MUST NOT 影响 text / tool_calls / finish_reason 的既有归一化,失败 MUST 降级为 `None`、不使 `complete` 失败。

#### Scenario: 从 message_start + message_delta 合成 usage

- **WHEN** 把含 `message_start.usage.input_tokens` 与 `message_delta.usage.output_tokens` 的 Anthropic SSE 字节流喂给累积器
- **THEN** 最终 `ModelResponse.usage` = `Some(Usage{ input_tokens, output_tokens })`,与等价语义的 OpenAI 响应归一化为同一形状

#### Scenario: 无 usage 降级为 None

- **WHEN** SSE 不含任何 usage 字段
- **THEN** `ModelResponse.usage` 为 `None`,text / tool_calls / finish_reason 的归一化不变
