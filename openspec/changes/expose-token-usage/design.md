## Context

现状(已对代码核实):

- `ModelResponse { text, tool_calls, finish_reason }`(`src/provider/mod.rs:41`),3 个 `pub` 字段、**无 `Default`**、全仓库裸字面量构造。
- wire 层其实收到 usage:Anthropic `message_start` 带 `input_tokens` / 初始 `output_tokens`、`message_delta` 带最终 `output_tokens`(见 `anthropic_stream.rs` fixture);OpenAI 流式**需** `stream_options.include_usage` 才在末尾 chunk 回 usage。两路累积器目前都**未提取** usage。
- 本 change 是 1.1 三步地基之一(另一为 `add-context-strategy`),**只暴露、不消费**。

权威次序:行为以 code / 测试为准。

## Goals / Non-Goals

**Goals:**
- 把真实 token 用量归一化进 `ModelResponse.usage`。
- OpenAI / Anthropic 两路 SSE 解析填充;Mock 可设。

**Non-Goals(本 change 明确不做):**
- 不触发任何压缩 / 截断 / summary(留 `add-token-compaction`)。
- 不引 tokenizer crate(用 provider 真实回传,零依赖;各家分词不一)。
- 不做 token 预估(压缩阶段如需预估再议)。
- 不碰 `ContextStrategy` / loop(那是 `add-context-strategy`,本 change 不依赖它)。

## Decisions

### ① Usage 形状:input/output 两字段,total 作方法

- `Usage { input_tokens: u32, output_tokens: u32 }`;`fn total(&self) -> u32 { input_tokens + output_tokens }`。**不存** total 字段,避免与 input+output 不一致。
- `ModelResponse.usage: Option<Usage>` —— `Option` 因:端点未开 `include_usage` / 字段缺失 / Mock 未设时**无**用量;不臆造 `0`(`0` 与「未知」语义不同,会误导压缩阈值)。

### ② Default 派生(抗加字段扩散 + 压低并行交叠)

- `ModelResponse` 与 `FinishReason` 派生 `Default`(`FinishReason::default() = Stop`)。理由:加 `usage` 后全仓库裸字面量构造点都要补字段;派生 `Default` 让构造点可 `..Default::default()` 兜未显式字段,**降低本次及未来加字段的扩散面**,也压低与 `add-context-strategy` 在 `agent/mod.rs` 测试构造点的整合成本。

### ③ OpenAI 流式 usage 需显式开启

- OpenAI chat completions 流式**默认不回 usage**,必须在请求体加 `stream_options: { include_usage: true }`;开启后流末尾多一个 `choices: []` 且带 `usage` 的 chunk。累积器需识别该 usage-only chunk、取 `prompt_tokens` / `completion_tokens`,且 **MUST NOT** 把它误当文本 / 工具增量。
- 既有「SSE 流式累积」「OpenAiProvider 实 HTTP 请求」requirement 的 scenario(text / tool_calls / finish_reason / `stream:true`)行为不变;本项为**叠加**,故用 **ADDED** 新 requirement,不 MODIFY 既有(降低 byte-for-byte 风险)。

### ④ Anthropic 流式 usage 分两处合成

- Anthropic 在 `message_start.message.usage`(input_tokens + 初始 output_tokens)与 `message_delta.usage`(最终累计 output_tokens)分别给。累积器取 `message_start` 的 `input_tokens` 与 `message_delta` 的 `output_tokens` 合成 `Usage`;任一字段缺失记 `0`,**两类事件均无 usage 才为 `None`**(Anthropic 常态总带 `message_start.usage`)。

### ⑤ 解析失败不致命

- usage 解析失败 / 字段缺失 **MUST** 降级为 `None`,**MUST NOT** 使整个 `complete` 失败——usage 是辅助计量,缺它不该让一轮对话挂掉。既有 `ProviderError::Decode` 致命语义仅针对 text / tool_calls 主体。

## Risks / Trade-offs

- **`include_usage` 兼容性**:个别 OpenAI 兼容端点(本地 llama.cpp 等)可能不认 `stream_options` 或不回 usage → `usage = None`,功能降级但不报错。压缩阶段对 `None` 必须有兜底(无用量时退回不压 / 字符预估)。**本项目默认 provider DeepSeek 实测支持 usage 回传**。
- **Default 给 FinishReason 设 Stop 默认**:可能掩盖「忘设 finish_reason」;但所有真实构造点都显式设,`Default` 仅作 `..` spread 兜 `usage`,风险低。
- **与 `add-context-strategy` 的 agent/mod.rs 交叠**:已由 `Default` 派生 + 「B 不构造 ModelResponse」双向压制;merge 由主 agent 收口。
