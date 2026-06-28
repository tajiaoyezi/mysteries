## ADDED Requirements

### Requirement: 响应携带 token 用量

`ModelResponse` SHALL 携带 `usage: Option<Usage>`,暴露该轮真实 token 用量;`Usage` MUST 含 `input_tokens` 与 `output_tokens`(均 `u32`),并以方法 `total()` 给出二者之和,MUST NOT 存独立 total 字段(避免与 `input + output` 不一致)。当 provider 未回传可用用量(端点不支持 / 字段缺失 / Mock 未设)时,`usage` MUST 为 `None`,MUST NOT 臆造 `0` 或 panic。`Usage` 解析失败 MUST 降级为 `None`、MUST NOT 使 `complete` 失败(用量为辅助计量;既有 text / tool_calls 的 `Decode` 致命语义不变)。`ModelResponse` 与 `FinishReason` SHALL 派生 `Default`(`FinishReason::default()` = `Stop`),使既有与未来构造点可经 `..Default::default()` 兜未显式字段。

#### Scenario: usage 经 ModelResponse 透传

- **WHEN** 一个带 `usage: Some(Usage{ input_tokens, output_tokens })` 的 `ModelResponse` 由 provider 产出
- **THEN** 调用方可从 `ModelResponse.usage` 读到该 `Usage`,且 `total()` 等于 `input_tokens + output_tokens`

#### Scenario: 无用量为 None

- **WHEN** provider 响应不含可用 token 用量
- **THEN** `ModelResponse.usage` 为 `None`,`complete` 仍正常返回完整 text / tool_calls / finish_reason

#### Scenario: MockProvider 可设用量

- **WHEN** 以脚本 `[ModelResponse{ usage: Some(..), .. }]` 构造 Mock 并调用 `complete`
- **THEN** 返回的 `ModelResponse` 携带该预设 `usage`(Mock 无需新增 API,经预置响应透传)
