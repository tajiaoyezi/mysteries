# anthropic-transport Specification

## Purpose
定义 Anthropic Messages API 的接入传输层:`AnthropicProvider` 的 HTTP 请求与鉴权(`x-api-key` + `anthropic-version`)、内核 `Message[]` 到 Messages 请求体的序列化,以及 SSE 事件流到 `ModelResponse`(含 `Usage`)的流式累积。关键立场是协议差异止于传输层:累积结果与 OpenAI 归一化为同一 `ModelResponse` / `ToolCall` / `Usage` 形状,SSE 累积与错误分类同 reqwest 解耦、可离线单测,超时与重试复用与 OpenAI 同一套 transport 逻辑。`Provider` trait 与凭据解析属 provider-abstraction / credential-source,provider 的选择与装配属 cli-runtime;本域仅覆盖 Anthropic 协议自身的 wire 细节。
## Requirements
### Requirement: AnthropicProvider 实 HTTP 请求与鉴权

系统 SHALL 提供 `AnthropicProvider`,impl `Provider`,持可配 `base_url`(默认 `https://api.anthropic.com`)+ `CredentialChain`。`complete` MUST POST 到 `{base_url}/v1/messages`,用 `x-api-key`(凭据,经 `expose_secret()`)+ `anthropic-version` 头(**非** `Authorization: Bearer`);明文 MUST NOT 入错误 / 日志。`resolve("anthropic")` 为 `None` → 立即 `ProviderError::Auth`(致命,不发 HTTP、不重试)。超时与重试 MUST 复用与 OpenAI 同一套 transport 逻辑(`with_retry` / `classify`)。

#### Scenario: 凭据缺失立即致命且不触网

- **WHEN** `CredentialChain` 对 `"anthropic"` 解析为 `None`,调用 `complete`
- **THEN** 返回 `Err(ProviderError::Auth)`,未发起任何 HTTP、未重试

#### Scenario: 鉴权头为 x-api-key + anthropic-version

- **WHEN** 凭据命中,构造请求(离线)
- **THEN** 请求带 `x-api-key`(= 凭据明文,经 `expose_secret()`)与 `anthropic-version` 头,**不**带 `Authorization: Bearer`

### Requirement: Anthropic 请求归一化(Messages API)

系统 SHALL 将内核 `Message[]` 序列化为 Anthropic Messages 请求体(§5.1/§5.5):`System` → 顶层 `system` 字段(**非** message role);`User` / `Assistant` → `messages` 项;`Assistant{text, tool_calls}` → assistant 消息的 content blocks(文本块 + `tool_use` 块,各 `tool_use` 含 `id`/`name`/`input`);`ToolResult{call_id, content, is_error}` → `user` 消息内 `tool_result` block(`tool_use_id` = call_id);工具定义用 `input_schema`(**非** OpenAI 的 `parameters`);`max_tokens` MUST 输出(Anthropic 必填)。

#### Scenario: 四类消息映射到 Anthropic 结构

- **WHEN** 一组依次含 System / User / Assistant(带 tool_calls)/ ToolResult 的会话被序列化
- **THEN** `system` 为顶层字段;Assistant 的 tool_calls 为 `tool_use` 块;ToolResult 为 user 消息的 `tool_result` 块(`tool_use_id` 正确回填);工具用 `input_schema`;请求体含 `max_tokens`

### Requirement: Anthropic SSE 流式累积归一化

系统 SHALL 提供与 reqwest 解耦的 Anthropic SSE 累积逻辑(impl 共享 `SseAccumulator`,吃字节 chunk):解析 `message_start` / `content_block_start` / `content_block_delta` / `content_block_stop` / `message_delta` / `message_stop` 事件;`text_delta` 文本增量 MUST 即时经 `DeltaSink` 推出;`input_json_delta` 工具输入片段 MUST 按 content block index 累积;`message_stop` MUST 落成 `ModelResponse`。累积结果 MUST 归一化为与 OpenAI **同一** `ModelResponse` / `ToolCall` 形状(`tool_use` → `ToolCall{id,name,arguments:Value}`,`stop_reason` → `FinishReason`),tool 输入片段拼接后非法 JSON → `ProviderError::Decode`。

#### Scenario: 文本与 tool_use 累积归一化

- **WHEN** 把一段 fixture Anthropic SSE 字节流(含 `text_delta` 多段、一个 `tool_use` 的 `input_json_delta` 分片、`message_stop`)喂给累积器
- **THEN** `DeltaSink` 按序收到文本增量;最终 `ModelResponse` 的 `text` / `tool_calls`(`arguments` 已解析为 `Value`)/ `finish_reason` 与等价语义的 OpenAI 响应归一化为**同一**形状

#### Scenario: 跨 chunk 边界缝合

- **WHEN** fixture 字节流在任意位置(含事件中段)被切成多个 chunk 依次喂入
- **THEN** 累积结果与不切分一致(内部缓冲跨 chunk 缝合,不丢不重)

#### Scenario: 非法 tool 输入

- **WHEN** 某 `tool_use` 的 `input_json_delta` 片段拼接后不是合法 JSON
- **THEN** 落成时返回 `ProviderError::Decode`(不 panic)

### Requirement: Anthropic token 用量解析

Anthropic SSE 累积逻辑 SHALL 从 `message_start` 事件的 `message.usage.input_tokens` 与 `message_delta`(`message_stop` 前最后一次)的 `usage.output_tokens` 收集 token 用量,合成 `ModelResponse.usage = Some(Usage{ input_tokens, output_tokens })`,归一化为与 OpenAI **同一** `Usage` 形状;任一字段缺失记 `0`,两类事件均无 usage 才为 `None`。usage 解析 MUST NOT 影响 text / tool_calls / finish_reason 的既有归一化,失败 MUST 降级为 `None`、不使 `complete` 失败。

#### Scenario: 从 message_start + message_delta 合成 usage

- **WHEN** 把含 `message_start.usage.input_tokens` 与 `message_delta.usage.output_tokens` 的 Anthropic SSE 字节流喂给累积器
- **THEN** 最终 `ModelResponse.usage` = `Some(Usage{ input_tokens, output_tokens })`,与等价语义的 OpenAI 响应归一化为同一形状

#### Scenario: 无 usage 降级为 None

- **WHEN** SSE 不含任何 usage 字段
- **THEN** `ModelResponse.usage` 为 `None`,text / tool_calls / finish_reason 的归一化不变

### Requirement: Anthropic 思考请求映射、流式解析与多轮原样回传

`anthropic_wire` 的请求序列化 SHALL 依 `anthropic_thinking_capability(model)` 与请求 `Depth` 映射思考字段:
- `Adaptive` 且 `Depth≠Off` → 顶层 `thinking={type:"adaptive", display:"summarized"}` + **独立顶层** `output_config={effort: <depth capped>}`;
- `Adaptive` 且 `Depth=Off` 且 `can_disable` → `thinking={type:"disabled"}`;`Depth=Off` 且 `!can_disable`(恒开模型)→ 仅 `output_config={effort:"low"}`、不发 thinking;
- `Budget` 且 `Depth≠Off` → `thinking={type:"enabled", budget_tokens: clamp(max_tokens×ratio, 1024, max_tokens-1), display:"summarized"}`(`budget_tokens` MUST `< max_tokens` 且 `≥1024`);**`max_tokens` 为 `None` 或 `<1025` 时 MUST 不发 budget_tokens**(退回省略 thinking),以免 `clamp(_,1024,max_tokens-1)` 在 `min>max` 时 u32 panic;`Depth=Off` → 省略 thinking;
- `None` → 不动 body。
系统 MUST NOT 设置 `tool_choice` 强制工具(any/tool)以免与思考不兼容(默认 auto)。

`serialize_request` 的 Assistant 分支 SHALL 把该消息 `thinking: Vec<ThinkingBlock>` 的每块作为 content 数组**首批**元素、排在 text/tool_use **之前**、**逐字节原样**回传:`{type:"thinking", thinking:<text>, signature:<sig>}`,`redacted` 块作 `{type:"redacted_thinking", data:<sig>}`;思考载体为空则维持引入前的 content 结构。此为带 tool_use 多轮不被 400 拒的硬约束。

`anthropic_stream` SHALL 解析思考流:`content_block_start` 识别 `type=="thinking"|"redacted_thinking"` 建块;`content_block_delta` 处理 `thinking_delta`(累积 text 并调 `on_thinking` 流式外发)与 `signature_delta`(累积 signature);`finish` 把累积块保序推入 `ModelResponse.thinking`(含 `thinking` 为空文本的 omitted 块)。

#### Scenario: 当代模型 adaptive+effort 请求体

- **WHEN** model=`claude-opus-4-8`(Adaptive)、`Depth::Medium`,序列化请求
- **THEN** body 含 `thinking={type:"adaptive",display:"summarized"}` 与顶层 `output_config={effort:"medium"}`;不含 `budget_tokens`

#### Scenario: 老模型 budget 请求体且 budget<max_tokens

- **WHEN** model=`claude-haiku-4-5`(Budget)、`Depth::High`、`max_tokens=16000`
- **THEN** body 含 `thinking={type:"enabled",budget_tokens:N,display:"summarized"}` 且 `1024 ≤ N < 16000`

#### Scenario: max_tokens 过小/None 时 Budget 分支不发 budget_tokens

- **WHEN** model=`claude-haiku-4-5`(Budget)、`Depth::High`、`max_tokens=1000`(或 `None`)
- **THEN** 不 panic、body 省略 thinking(不发 budget_tokens)

#### Scenario: Off 分模型处理

- **WHEN** `Depth::Off` 对 `claude-sonnet-5`(can_disable) vs `claude-fable-5`(恒开)
- **THEN** 前者 `thinking={type:"disabled"}`;后者不发 thinking、发 `output_config={effort:"low"}`

#### Scenario: 带 tool_use 的 Assistant 原样回传 thinking 块

- **WHEN** 序列化一条 `thinking` 非空且含 `tool_calls` 的 `Message::Assistant`
- **THEN** content 数组首元素为 `{type:"thinking",thinking,signature}`(字节一致)、其后才是 tool_use;signature 未被改动

#### Scenario: 流式累积 thinking 与 signature

- **WHEN** 依次喂 `thinking_delta`(文本分片)+ `signature_delta` + `content_block_stop`
- **THEN** `on_thinking` 被逐片调用;`finish` 后 `ModelResponse.thinking` 含该块、text 拼全、signature 完整

