## Context

bootstrap 采 Option A 只做 OpenAI 归一化,把 live 传输推迟到「下一个 change」(bootstrap design D3);`add-credential-chain` 已实现 + archived,交付了 `CredentialChain::new(vec![Box<dyn CredentialSource>])` + `resolve("openai") -> Option<SecretString>`(`secrecy = 0.10.3`,`ExposeSecret::expose_secret()`,Debug 脱敏)。本 change 在此之上交付真实 `OpenAiProvider` 与 §5.2 的流式 / 超时 / 重试,**消费**该链、不再做凭据。

约束:Rust 自实现(禁第三方 Agent SDK);Provider 归一化 / 传输属 CLAUDE.md「强制 TDD」的 headless 内核;测试**不依赖真实网络**(fixture 字节流 / 注入 / 假链 / 虚拟时间);真实调用仅 `#[ignore]` gated。权威次序 code / 编译器 / 测试 > spec > Agent 推断,冲突显式标注。本 change 不触及 UI。

## Goals / Non-Goals

**Goals:**

- 真实 `OpenAiProvider`:reqwest POST 到可配 `base_url`,鉴权取自 `CredentialChain`,凭据缺失 → `Auth` 致命。
- SSE 流式累积(§5.2)做成**与 reqwest 解耦的纯逻辑**,可对 fixture 字节流离线单测。
- per-attempt 超时 + 429/5xx/网络 指数退避重试;`ProviderError::{Auth, RateLimited, Timeout}` 落地。
- 错误分类 / 重试驱动 / SSE 累积均离线 TDD;真实调用仅 gated smoke。

**Non-Goals(留后续 change):**

- Anthropic、TUI、配置分层(`base_url`/timeout/重试上限的最终可配化 —— 本 change 用默认常量)。
- main 接多轮 `Agent` Loop + stdin y/n decider。
- §5.1 本地非 function-calling `tool_mode` 降级。
- 非流式响应路径与 `wire::parse_response` 的激活(见 D3)。

## Decisions

- **D1 模块布局。** 新建 `src/provider/openai.rs`(`OpenAiProvider`)+ `src/provider/stream.rs`(SSE 累积纯逻辑);`provider/mod.rs` 注册 `pub mod openai; pub mod stream;`。`complete` 调 `wire::serialize_request` → **激活其 dead_code**。备选:全塞 openai.rs(弃:SSE 累积是可独立测的纯逻辑,分文件利于 fixture 单测与未来 Anthropic 复用同形累积)。

- **D2(偏离用户描述,显式标注)`OpenAiProvider` 不持 `model` 字段,读 `ModelRequest.model`。** 代码现状:`Agent::run` 设 `req.model = self.model`、`run_single_turn` 设 `req.model = DEFAULT_MODEL`、`wire::serialize_request` 序列化 `req.model` —— `ModelRequest.model` 已是既有 source of truth。provider 再持一个 `model` 会与既有调用语义冲突(到底以谁为准)。按权威次序(code > 描述)读 `req.model`。备选:provider 持 model 并覆盖 req.model(弃:双源、易混)。

- **D3(显式 loose end)本 change 走流式 only;`wire::parse_response`(非流式)不激活。** §5.1 要求文本 delta 实时经 `DeltaSink` 推 UI,流式是产品核心路径;非流式当前**无消费者**(simplicity,不投机)。故 `complete` 恒 `stream:true` + SSE 累积器产出 `ModelResponse`;`wire::parse_response` 保持 dead_code,留给未来「非流式 / `tool_mode` 降级」change 激活。**显式标注**而非静默(权威次序:surface,不藏)。`stream:true` 在传输层附加(不改 `wire`,保持 wire 为「消息归一化」与 stream 无关)。

- **D4 自解 SSE,不引 `eventsource-stream`。** 累积器吃 `&[u8]` chunk、内部行缓冲跨 chunk 缝合、解析 `data:` / `[DONE]`、忽略 `event:` / 注释行;文本 delta → sink,`tool_call` 按 `index` 累积(`BTreeMap<usize, Partial>`,首个非空取 id/name、`arguments` 片段拼接),终态拼接后 JSON parse(复用 wire 的「JSON 字符串 → `Value`、非法 → `Decode`」语义)。**理由**:纯字节输入 = 可对 fixture 离线单测,且少一个依赖。备选:`eventsource-stream`(弃:把解析耦进 `Stream`,fixture 字节测试更难,且多依赖;用户已许「或自解 SSE」)。

- **D5 错误分类纯函数(吃 `u16` / 抽象 kind,不吃 reqwest 类型)。** `401`/`403` → `Auth`(致命);`429` → `RateLimited`(重试);`5xx` → `RateLimited`(重试);其他 `4xx` → `Transport`(致命);body JSON/SSE 解析失败 → `Decode`(致命);传输错误 kind:timeout → `Timeout`(重试)、connect/网络 → 可重试 `Transport`。**§9 仅 4 变体,5xx 归 `RateLimited` 是命名折中**(其「可重试」语义正确,非字面限流)—— 显式标注。**理由**:纯函数 → 离线穷举各 status/kind;与 reqwest 解耦。备选:在变体上挂 `is_retryable()`(弃:`Transport` 既可重试[网络]又致命[4xx],单凭变体判不准;分类放「有富信息的 HTTP 边界」更准)。

- **D6 重试驱动可注入 + 虚拟时间。** `with_retry(attempt, policy)`,`attempt: FnMut() -> impl Future<Output = Result<T, ProviderError>>`;per-attempt `tokio::time::timeout` 包 `attempt`;按 D5 分类决定重试与指数退避。**测试**注入脚本化 `attempt`(`RateLimited×2` then `Ok` / `Auth` / 全 `RateLimited` 耗尽)+ `tokio::time` 暂停推进(虚拟时间,免真实 sleep)→ 即时、离线。**理由**:retry/backoff 与 reqwest 解耦,正是「可独立审 / 测」的那块——**在一个 change 内以独立模块 + 任务组隔离即可,无需拆 change**(回应「重试 / 错误分类是否拆出」:其复用 / 可测性在此已满足)。

- **D7 凭据缺失短路。** `complete` 先 `self.credentials.resolve("openai")`,`None` → 立即 `Err(Auth)`,不进重试、不发 HTTP;`expose_secret()` 仅在构造 Bearer 头时调用一次(明文出现点集中、可审计)。离线测:空 `CredentialChain::new(vec![])` 或注入空 env lookup。

- **D8 默认常量,非配置。** `base_url` 默认、per-attempt timeout、`max_retries`、退避 base 用模块内 `const` 合理默认;最终可配化(随 `Config.timeout_secs` 等,§7)留配置分层 change。**理由**:配置分层明确不含,不投机配置面。

- **D9 live smoke gating。** `#[ignore]` 的 live 测试:运行时若 `OPENAI_API_KEY` 缺失则 early-return(skip 不 fail);否则构造真实 `OpenAiProvider`(默认 `base_url`、`EnvCredentialSource`)+ `ModelRequest{ model = env OPENAI_MODEL 或 "gpt-4o-mini" }` → 直接 `complete`,断言非空文本 / sink 收增量。默认 `cargo test` 因 `#[ignore]` 排除,显式 `cargo test -- --ignored` 才跑。**绝不进默认 cargo test**。main 单轮接真实 provider 列**可选**(需 `run_single_turn` 接受 model,轻微 `conversation` 改),默认不做。

## Risks / Trade-offs

- **[流式 only,`parse_response` 仍 dead]** → D3 显式标注;未来非流式 / `tool_mode` change 激活;当前无行为损失。
- **[§9 4 变体不覆盖 5xx 字面语义]** → D5 标注 `5xx → RateLimited` 命名折中;「可重试」语义正确;未来若需可加 `Server` 变体。
- **[reqwest 请求 / 响应难离线断言]** → 把请求体构造、错误分类、重试驱动、SSE 累积全做成**纯 seam** 离线测;真实 send 仅 gated smoke 兜底(fixture 取自 OpenAI 官方 SSE 样例)。
- **[跨 change 依赖 A]** → A 已实现 + archived(本轮已核 `src/credential/mod.rs` 与主 specs `credential-source`),B 直接消费**真实** API,无猜测;apply 顺序 A→B 已满足。
- **[reqwest TLS / 体积]** → `default-features = false` + `rustls-tls`,避免 native-tls 的系统 OpenSSL 依赖,跨平台一致。

## Migration Plan

纯新增 provider + 模块;不改 `main` 既有 `MockProvider` 单轮路径(除非选 D9 的 optional smoke)。`error.rs` 增 3 变体为向后兼容(既有 `Transport`/`Decode` 不变,既有断言不受影响)。回滚 = revert 本 change 提交。

## Open Questions

- 非流式 `wire::parse_response` 的激活时机(未来非流式 / `tool_mode` 降级 change)。
- `5xx` 是否值得未来独立 `Server` 变体(现折中归 `RateLimited`)。
- per-attempt timeout / `max_retries` / 退避 base 的默认值,最终由配置分层 change(§7 `Config.timeout_secs`)接管;本 change 仅定 `const` 默认。
