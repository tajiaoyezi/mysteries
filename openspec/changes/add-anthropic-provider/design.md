## Context

`Provider` 抽象自 bootstrap 即为双 wire format 设计(§5.1),但至今只有 OpenAI 一个真实实现。本 change 补 Anthropic provider,兑现 §13「换实现非改架构」。以 `add-openai-live-transport`(已 archived)为模板,**复用其 transport-无关逻辑**。用户已确认:**抽公共 transport helper、单 change**。

现状(real code,openai.rs):`with_retry<T,F,Fut>(policy, attempt)`、`RetryPolicy`、`ErrorClassification`、`TransportFailure`/`TransportErrorKind`、`backoff_delay`、`classify(failure)`(映射 agnostic,但错误串嵌 `"OpenAI"`)、`classify_reqwest_error` 均 transport-无关、`pub`;`StreamAccumulator`(stream.rs)已是 `push_chunk(&mut, &[u8], &dyn DeltaSink) -> Result<Option<ModelResponse>>` + `finish(&self) -> Result<ModelResponse>` 形状。OpenAI 专属 = `build_request_body`(wire+stream)、Bearer 头、`/chat/completions`、`StreamAccumulator`。约束:headless 内核强制 TDD、不依赖真实网络/FS、零新依赖。

## Goals / Non-Goals

**Goals:**

- 提取共享 `transport.rs`(retry/timeout/classify/accumulate-loop),openai 重构复用(零回归)。
- `AnthropicProvider` + Anthropic wire(§5.1/§5.5)+ Anthropic SSE 累积,归一化到**同一** `ModelResponse`/`ToolCall`。
- 凭据 anthropic arm + select_provider anthropic arm。
- 全离线 red-green;真实仅 `#[ignore]`。

**Non-Goals(留后续):**

- §5.1 `tool_mode` 本地降级(Anthropic 原生 tool use,不需要)。
- 内置命令、`ToolOutcome.exit`、step5 其余收尾。

## Decisions

- **D1 共享 `provider/transport.rs`(behavior-preserving 提取)。** 把 openai.rs 的 transport-无关项搬来:`with_retry` / `RetryPolicy` / `ErrorClassification` / `TransportFailure`·`TransportErrorKind` / `backoff_delay` / `classify_reqwest_error` + **新** `SseAccumulator` trait(`push_chunk` / `finish`)+ 泛型 `accumulate_stream<S,B,E,A: SseAccumulator>`。openai.rs 改 import 共享层,`StreamAccumulator impl SseAccumulator`。**openai 既有测试零回归 = 验收闸**(重构不改行为)。备选:Anthropic 从 `openai::*` 直接 import(弃:把 Anthropic 耦到 openai 模块,语义错乱)。

- **D2 `classify` 加 provider label。** `classify(failure, provider_label: &str)` —— 映射(401/403→Auth、429/5xx→RateLimited、Timeout/Network/Decode…)不变,仅 `Transport`/`Decode` 的消息串用 label(openai 传 `"OpenAI"` → 串与原逐字一致零回归;anthropic 传 `"Anthropic"`)。备选:消息串去掉 provider 前缀(弃:可能动 openai 既有断言,破零回归)。

- **D3 Anthropic wire 专属(`anthropic_wire.rs`)。** `serialize_request(req) -> Value`:`system` 收集 `Message::System` 为顶层字段;`messages` 收 User/Assistant;`Assistant.tool_calls` → assistant content 的 `tool_use` 块(`{type:"tool_use", id, name, input}`);`ToolResult` → `user` 消息 content 的 `tool_result` 块(`{type:"tool_result", tool_use_id, content, is_error?}`);`tools[]` 用 `input_schema`;`max_tokens` 必填(取 `req.max_tokens`,缺则默认常量)。**纯函数,red-green 断言 body 形状**(对照 §5.1 差异表 Anthropic 侧三项 + max_tokens/input_schema)。

- **D4 Anthropic SSE 累积器(`anthropic_stream.rs`,impl `SseAccumulator`)。** 解析 `event:`+`data:` 对(Anthropic SSE 带 event 行):`content_block_start`(记 block 类型/index)、`content_block_delta`(`text_delta`→`sink.on_text` + `input_json_delta`→按 index 累积 tool 输入串)、`content_block_stop`、`message_delta`(`stop_reason`)、`message_stop`→落 `ModelResponse`。`tool_use` block → `ToolCall{id,name,arguments}`(输入串拼接后 parse,非法→`Decode`);`stop_reason` 映射 `FinishReason`(`end_turn`→Stop、`tool_use`→ToolCalls、`max_tokens`→Length、其余→Other)。内部行缓冲跨 chunk 缝合。**fixture 字节流 red-green**,断言**与 OpenAI 同一** `ModelResponse`/`ToolCall`。

- **D5 `AnthropicProvider`(`anthropic.rs`)。** 持 `base_url`(默认 `https://api.anthropic.com`)+ `CredentialChain` + `reqwest::Client` + `RetryPolicy`;`complete`:`resolve("anthropic")` 缺失→`Err(Auth)`;`x-api-key`+`anthropic-version` 头;POST `{base_url}/v1/messages`;`with_retry` 包裹;`accumulate_stream` + `AnthropicAccumulator`。`name()="anthropic"`。cred-missing→Auth 离线测;真实 send 仅 `#[ignore]`。

- **D6 凭据 + 接线(各 +1 arm)。** `EnvCredentialSource::resolve` 加 `"anthropic" => "ANTHROPIC_API_KEY"`(red-green);`select_provider` 的 `Anthropic` arm 改构造 `AnthropicProvider`(base_url 有则 `new` 无则 `default`,offline)。

- **D7 gated live smoke。** `#[ignore]` + `ANTHROPIC_API_KEY` 缺则早退;构造真实 `AnthropicProvider` + 真实 model(env `ANTHROPIC_MODEL` 或字面)→ `complete` 断言非空。绝不进默认 `cargo test`。

## Risks / Trade-offs

- **[重构 archived openai.rs]** 提取共享层动到工作中的 openai.rs/stream.rs → 缓解:behavior-preserving;**openai 既有测试全程保持绿 = 闸**;`classify` 加 label 保串不变。
- **[Anthropic SSE 与 OpenAI 形状差异大]** event 行 + 多事件类型 + block index → 缓解:fixture 取 Anthropic 官方 SSE 样例;`SseAccumulator` trait 统一出口;红灯断言归一化到同一 `ModelResponse`/`ToolCall`。
- **[max_tokens 必填]** OpenAI 选填、Anthropic 必填 → 缓解:D3 取 `req.max_tokens`,缺则默认常量(文档化),保证总输出。
- **[真实 wire 细节偏差]** fixture 离线 → 缓解:gated `#[ignore]` smoke 兜底真实端点。

## Migration Plan

提取 `transport.rs`(openai 重构,零回归);新增 anthropic 三文件;`credential`/`app` 各 +1 arm。`provider-abstraction` 不动;openai-transport 无 spec 变更(纯重构)。回滚 = revert(openai 复原内联 transport、移除 anthropic)。无数据迁移。

## Open Questions

- `accumulate_stream` 的 provider 错误串(现 "OpenAI stream error")→ 随 label 化或泛型 message 参数;实现期定,保 openai 零回归。
- Anthropic 默认 `max_tokens` 常量取值(req 缺失时)—— 实现期定合理默认,后续随 config 可调。
- `anthropic-version` 头取值(API 版本串)—— 实现期 pin 一个稳定版本,后续可配。
