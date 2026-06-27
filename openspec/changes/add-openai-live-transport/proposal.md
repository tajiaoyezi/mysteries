## Why

bootstrap(Option A)只交付了 OpenAI *归一化*(`wire::serialize_request` / `parse_response`),刻意把 live 传输推迟(bootstrap design D3);`add-credential-chain` 已交付 `CredentialChain`。现在补齐技术方案 §12 第 1 步的「OpenAI 兼容实现」实传输与 §5.2 的「流式 / 超时 / 重试」,让 Agent 真正能与 OpenAI(及兼容 / 本地端点)通话 —— 这是 headless 内核**唯一仍由 Mock 驱动**的缝。凭据已由 A 交付,本 change **消费** `CredentialChain`、**不再做凭据**。

## What Changes

- 新建 `OpenAiProvider`(`src/provider/openai.rs`),impl `Provider`:持 `base_url`(可配,默认 `https://api.openai.com/v1`,同时支持本地 / 自定义端点)+ `CredentialChain`(消费 A)+ `reqwest::Client`。
- `complete`:
  - 请求体复用既有 `wire::serialize_request`(**激活其 dead_code**)并在传输层加 `"stream": true`;`reqwest` POST 到 `{base_url}/chat/completions`;
  - `Authorization: Bearer <key>`,`key = CredentialChain.resolve("openai")?` 经 `expose_secret()`;**凭据缺失 → `ProviderError::Auth`(§9 致命,不发 HTTP、不重试)**。
- 新建 SSE 累积器(`src/provider/stream.rs`,§5.2),**纯逻辑、吃字节 chunk**:逐 `data:` 解析,文本增量经 `DeltaSink` 即时推出,`tool_call` 增量按 `index` 累积(id / name / arguments 片段),`[DONE]` / 流结束落成 `ModelResponse`;arguments 片段拼接后 JSON parse,非法 → `Decode`。**自解 SSE,不引 `eventsource-stream`**。
- 超时 + 重试(§5.2):per-attempt `tokio::time::timeout`;429 / 5xx / 网络错误指数退避重试至上限;`Auth` 及其他致命快速失败不重试;重试耗尽返回最后错误(对 Agent loop 即致命)。
- HTTP 错误分类做成**纯函数**(status `u16` / 抽象传输错误 kind → 重试 vs 致命 + `ProviderError` 变体),与 reqwest 解耦、离线可测。
- `error.rs` 新增 `ProviderError::{Auth, RateLimited, Timeout}`(bootstrap D9 已预告;现在才有构造点),保持 `PartialEq, Eq`。
- gated live smoke:`#[ignore]` + `OPENAI_API_KEY` 守卫的真实调用测试,直接 `complete`(带真实 model);**绝不进默认 cargo test**。

**明确不含**(留后续 change):

- Anthropic provider、TUI、配置分层(TOML user/project,含 `base_url` / timeout / 重试上限的最终可配化 —— 本 change 用模块内**合理默认常量**)。
- main 接多轮 `Agent` Loop + stdin y/n decider(留 config/assembly change)。
- §5.1 本地非 function-calling 的 `tool_mode` 降级路径(留;本 change 仅原生 `tools`)。
- **非流式响应路径**:本 change 走**流式**(§5.1 实时 `DeltaSink` 为产品要求);`wire::parse_response`(非流式)**本 change 不激活**,留给未来非流式 / `tool_mode` 降级 change(显式标注,见 design D3)。
- 把 `OpenAiProvider` 接进 `main` / `run_single_turn`:后者现硬编码 `model = "mock-model"`,接真实 provider 需 model thread-through(轻微 `conversation` 改动)—— 列为**可选** stretch,默认不做,避免 scope creep。

**显式偏离(权威次序 code > 描述)**:`OpenAiProvider` **不**持 `model` 字段。`ModelRequest.model` 已是既有 source of truth(`Agent` 设 `req.model = self.model`、`wire::serialize_request` 序列化 `req.model`);provider 读 `req.model` 即可,加字段会与既有调用语义不一致。见 design D2。

本 change 不触及 UI,故不涉及 `设计规范/` 引用。

## Capabilities

### New Capabilities

- `openai-transport`: 真实 OpenAI 兼容传输 —— `OpenAiProvider`(reqwest POST 到可配 `base_url`、`CredentialChain` 鉴权、凭据缺失致命)、SSE 流式累积(§5.2)、per-attempt 超时与指数退避重试。

### Modified Capabilities

- `provider-abstraction`: ADDED —— `ProviderError` 增补 `Auth`(致命)/ `RateLimited` / `Timeout`(可重试)变体,与既有 `Transport` / `Decode` 并列,为传输层提供 §9「可恢复 vs 致命」的错误词汇。既有 Provider / DeltaSink / 归一化 / Mock 等 requirement 不变。

## Impact

- **新增代码**:`src/provider/openai.rs`、`src/provider/stream.rs`;`provider/mod.rs` 注册二者;`error.rs` +3 变体。**激活** `wire::serialize_request`(其 dead_code 消解)。
- **新增依赖**(§11):`reqwest = "0.12"`(`default-features = false`,`json` / `stream` / `rustls-tls`;当前 lock 解析 `0.12.28`;理由:HTTP POST + `bytes_stream()` + 跨平台 TLS,避免 `native-tls` 系统依赖)、`futures-util = "0.3"`(`default-features = false`,`std`;当前 lock 解析 `0.3.32`;理由:仅取 `StreamExt::next` 驱动 reqwest 字节流,不引完整 `futures`)。**不引** `eventsource-stream`(自解 SSE)。`secrecy` 已由 A 引入(本 change 用 `ExposeSecret`),不重复加;`tokio` 启用 `time` + `test-util`(理由:per-attempt timeout 与虚拟时间退避测试)。
- **构建 / 测试**:`cargo build` 通过(`wire::parse_response` 仍 dead_code 为预期,见 design D3);SSE 累积 / 错误分类 / 重试 / 凭据缺失→Auth 全部**离线** TDD(fixture 字节流 / 注入 / 假 `CredentialChain` / 虚拟时间);真实调用仅 `#[ignore]` gated。`cargo test` 默认全绿且**不触网**。
- **下游契约**:`OpenAiProvider` 是首个真实 `Provider`;后续 Anthropic 照此形(`wire` + stream accumulator + 同一 retry / classify)扩展。
- **现状影响**:`main` 仍走既有 `MockProvider` 单轮(除非选 optional smoke),对既有行为零影响。
