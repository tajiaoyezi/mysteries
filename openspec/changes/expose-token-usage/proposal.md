## Why

1.1 Token 压缩要判断「当前上下文烧了多少 token」才能决定何时触发压缩。最准、零依赖的来源是 provider 每次响应回传的**真实 usage**(OpenAI 的 `usage`、Anthropic `message_start` / `message_delta` 的 `input_tokens` / `output_tokens`)。但现状 `ModelResponse` 只暴露 `text` / `tool_calls` / `finish_reason`,wire 累积层收到 usage 却**丢弃了**。

本 change 把真实 token 用量归一化进 `ModelResponse`,作为 1.1 压缩(`add-token-compaction`)的**计量地基**。本 change **只暴露用量、不消费**——不触发任何压缩 / 截断,不依赖 `add-context-strategy`,可与其并行。

## What Changes

- **provider-abstraction**:`ModelResponse` 增 `usage: Option<Usage>`;新增归一化类型 `Usage { input_tokens: u32, output_tokens: u32 }`,`total()` 由二者之和给出(不存冗余 total 字段)。无可用用量时为 `None`(不臆造、不 panic)。`ModelResponse` 与 `FinishReason` 派生 `Default`(`FinishReason::default() = Stop`),抗本次及未来加字段的扩散。
- **openai-transport**:请求体增 `stream_options: { include_usage: true }`(OpenAI 流式默认不回 usage);SSE 累积识别流末尾 usage-only chunk,取 `prompt_tokens` / `completion_tokens` → `ModelResponse.usage`;缺失则 `None`。
- **anthropic-transport**:SSE 累积从 `message_start.usage.input_tokens` 与 `message_delta.usage.output_tokens` 收集 → `ModelResponse.usage`;缺失则 `None`。
- **MockProvider**:返回的预置 `ModelResponse` 自然携带其 `usage`(脚本可设),**无需新增 API**。

## Capabilities

### New Capabilities
<!-- 无新增 capability:全部为对既有 provider-abstraction / openai-transport / anthropic-transport 的追加。 -->

### Modified Capabilities
- `provider-abstraction`: **ADDED**「响应携带 token 用量」(`ModelResponse.usage` + `Usage` 类型 + Mock 透传 + 无用量为 None + Default 派生)。
- `openai-transport`: **ADDED**「OpenAI token 用量解析」(请求开 `include_usage` + 末尾 usage chunk 解析 + 缺失降级)。
- `anthropic-transport`: **ADDED**「Anthropic token 用量解析」(`message_start` + `message_delta` 合成 usage + 缺失降级)。

## Impact

- **code**:`src/provider/mod.rs`(`ModelResponse.usage` + `Usage` + `Default`)、`src/provider/openai.rs`(请求加 `stream_options`)、`src/provider/stream.rs`(OpenAI SSE 累积 usage)、`src/provider/anthropic.rs` / `anthropic_stream.rs`(Anthropic SSE 累积 usage);各处既有 `ModelResponse` 构造点补 `usage`(优先 `..Default::default()`)。
- **并行说明(与 `add-context-strategy` 同期)**:加 `usage` 字段会触及 `src/agent/mod.rs` **测试模块**里的 `ModelResponse` 构造点(补 `usage: None` / `..Default::default()`),这是与 `add-context-strategy` 的**唯一**文件交叠;两者逻辑独立、改动不相邻,合并由主 agent 收口。`Default` 派生即为压低该交叠面。
- **conversation / agent-loop / tui 不受影响**:仅在 provider 层叠加字段 + 解析;Message 归一化、loop 行为、TUI 渲染契约不变。
- **deps**:零新增(`serde` / `serde_json` 已在)。
