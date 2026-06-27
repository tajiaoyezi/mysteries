## Why

`Provider` 抽象从 bootstrap 起就是为「双 wire format」设计的(§5.1 归一化差异表),但至今只有 OpenAI 一个真实实现 —— 抽象还没被第二个 provider **兑现**。本 change 补齐 **Anthropic provider**:把 Anthropic Messages API 的请求 / 响应 / 流式归一化到**同一** `ModelRequest` / `ModelResponse` / `ToolCall`,内核(Agent Loop / 工具 / TUI)一行不改即可换用。这正是 §13「Anthropic = 换实现非改架构」的兑现,也是 1.0 step5 收尾的一块。以 `add-openai-live-transport` 为模板,**复用其 transport-无关的 retry/timeout/classify**。

## What Changes

- **提取共享 `provider/transport.rs`**(behavior-preserving 重构):把 openai.rs 里 transport-无关的 `with_retry` / `RetryPolicy` / `ErrorClassification` / `TransportFailure`·`TransportErrorKind` / `backoff_delay` / `classify(failure, provider_label)`(加 label 参数,openai 传 `"OpenAI"` → 错误串不变)/ `classify_reqwest_error` 搬来,并抽 `SseAccumulator` trait(`push_chunk` / `finish` —— openai 的 `StreamAccumulator` 直接 `impl`)+ 泛型 `accumulate_stream<A: SseAccumulator>`。openai.rs 改为 import 共享层(**既有 openai 测试零回归 = 闸**)。
- **`AnthropicProvider`**(`provider/anthropic.rs`,impl `Provider`):`base_url` 可配(默认 `https://api.anthropic.com`)+ `CredentialChain`;`complete` 用 `x-api-key` + `anthropic-version` 头(非 Bearer)POST 到 `{base_url}/v1/messages`;凭据缺失 → `ProviderError::Auth`;**复用** `with_retry` + `accumulate_stream` + Anthropic 累积器。
- **Anthropic wire**(`provider/anthropic_wire.rs`,§5.1/§5.5):`System` → 顶层 `system` 字段(非 message role);`User`/`Assistant` → `messages`;`Assistant.tool_calls` → `tool_use` content block;`ToolResult` → `user` 消息内 `tool_result` block;`tools` 用 `input_schema`(非 `parameters`);`max_tokens` 必填。
- **Anthropic SSE 累积**(`provider/anthropic_stream.rs`,§5.2,impl `SseAccumulator`):解析 `message_start` / `content_block_start` / `content_block_delta`(`text_delta` 即时推 `sink` + `input_json_delta` 按 block index 累积 tool 输入)/ `content_block_stop` / `message_delta` / `message_stop` → 归一化成**同一** `ModelResponse` / `ToolCall`(`stop_reason` 映射 `FinishReason`)。
- **凭据**:`EnvCredentialSource` 加 `"anthropic"` → `ANTHROPIC_API_KEY`(现仅 openai)。
- **接线**:`select_provider` 的 `Anthropic` arm 由 `Err(UnsupportedProvider)` 改真实 `AnthropicProvider`(offline 构造不触网)。
- **gated live smoke**:`#[ignore]` + `ANTHROPIC_API_KEY` 守卫(缺则早退),绝不进默认 `cargo test`。

### 4 点定夺

1. **复用边界** → 共享 `transport.rs`(retry/timeout/classify/accumulate-loop)+ Anthropic 专属(wire 序列化 / SSE 事件解析 / 头·url)。**抽公共 transport helper**(非各自实现),兑现 §13 抽象。
2. **capability 影响** → NEW `anthropic-transport`;**MODIFIED** `credential-source`(env 加 anthropic arm)+ `cli-runtime`(select_provider Anthropic arm),均 additive;`provider-abstraction` **不动**(只实现 trait);openai-transport **无 spec 变更**(共享提取是 behavior-preserving 重构,openai 测试零回归)。
3. **是否拆** → **单 change**(对标 openai-transport 也单 change;体量相当、内聚、零新依赖)。
4. **tool_mode 本地降级**(§5.1) → 明确**不含**(Anthropic 原生 tool use)。

本 change 不触及 UI,故不涉及 `设计规范/` 引用。

## Capabilities

### New Capabilities

- `anthropic-transport`: 真实 Anthropic Messages API 传输 —— `AnthropicProvider`(x-api-key/anthropic-version 鉴权、凭据缺失致命)、Anthropic wire 归一化(system 顶层 / tool_use·tool_result 块 / input_schema / max_tokens)、Anthropic SSE 累积,归一化到**同一** `ModelResponse`/`ToolCall`,复用共享 transport 的超时 / 重试。

### Modified Capabilities

- `credential-source`: MODIFIED —— `EnvCredentialSource` 环境变量映射**加** `"anthropic"` → `ANTHROPIC_API_KEY`(openai 映射不变,additive)。
- `cli-runtime`: MODIFIED —— `select_provider` 的 `Anthropic` arm 由 `Err(UnsupportedProvider)` 改为构造真实 `AnthropicProvider`(其余 OpenAi/Mock arm 不变)。

## Impact

- **新增代码**:`provider/transport.rs`(共享,从 openai.rs 提取)、`provider/anthropic.rs`、`provider/anthropic_wire.rs`、`provider/anthropic_stream.rs`;`provider/mod.rs` 注册;`provider/openai.rs`·`stream.rs` 重构(import 共享层 + `StreamAccumulator impl SseAccumulator`);`credential/mod.rs`(anthropic arm)、`app.rs`(select_provider arm)。
- **新增依赖**:**无**(`reqwest`/`secrecy`/`futures-util`/`tokio` 均已在)。
- **构建 / 测试**:headless 内核强制 TDD —— Anthropic wire 序列化 + SSE 累积(fixture 字节)+ classify/retry(mock 时间)+ cred/arm 全离线 **red-green**,归一化结果对**同一** `ModelResponse`/`ToolCall` 断言;真实 Anthropic 仅 `#[ignore]`(`ANTHROPIC_API_KEY` 缺则早退)。**既有 openai 测试零回归**(共享提取的闸)。`cargo test` 默认全绿、不触网。
- **里程碑**:`Provider` 抽象被第二个真实实现兑现;内核换 provider = 改 config 一行。1.0 仅剩内置命令 + 收尾。
