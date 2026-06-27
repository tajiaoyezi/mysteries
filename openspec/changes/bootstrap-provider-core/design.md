## Context

repo 暂无 Rust 代码,本 change 是 1.0 的奠基步(技术方案 §12 第 1 步)。目标是立起 headless 内核最底层的 `Provider` seam 并用单轮 stdout 链路验证。设计依据:§5.1(Provider 与协议归一化)、§5.5(Session 与规范 Message)、§10(测试策略)。

约束:Rust 实现;核心能力自实现,禁第三方 Agent SDK;Provider 归一化属 CLAUDE.md「强制 TDD」范围;权威次序 code/编译器/测试 > spec > Agent 推断,冲突须显式标注。

已与用户确认采 **Option A**:本 change 只做 OpenAI *归一化*,不做 live HTTP 传输与凭据;传输(reqwest + SSE,§5.2)与凭据链(§5.6)耦合,留下一个 change。

## Goals / Non-Goals

**Goals:**
- 确立 `Provider` / `DeltaSink` trait 与 `Message` / `ModelRequest` / `ModelResponse` / `ToolCall` / `FinishReason` / `ProviderError` 的契约形状(§5.1 / §5.5 / §9)。
- 实现并 TDD 覆盖 OpenAI 协议归一化(请求序列化 + 非流式响应解析)。
- 提供 `MockProvider` 作为测试基础设施与 stdout demo 驱动。
- 打通 IO 无关的 `run_single_turn` + 薄 `main`,实现单轮 stdout 对话。

**Non-Goals(留后续 change):**
- Agent Loop 多轮、工具系统、权限门、TUI、配置分层、凭据链、Anthropic provider。
- OpenAI live 传输:reqwest、SSE 流式解析与 tool 增量累积(§5.2)、超时/重试。
- 真实 endpoint 联调(归一化仅以 JSON fixture 验证)。

## Decisions

- **D1 模块布局遵循 §4,只建在用模块。** `provider/{mod,wire,mock}.rs`、`agent/{mod,message}.rs`、`error.rs`、`main.rs`。`agent/mod.rs` 本 change 仅 `pub mod message;`(无 loop / session)。备选:扁平单文件(弃:不立 §4 seam)、预建全部空模块(弃:投机,违背 simplicity)。
- **D2 单 crate(binary)。** 1.0 一个 crate,headless 由模块纪律保证(core 不 `use` tui;当前无 tui)。拆 core/tui crate 留后续。
- **D3 Option A 边界:归一化无传输。** 理由:传输与凭据天然耦合(无 key 无法真实调用),放一起做更干净;归一化用 fixture 即可离线、确定性、全 TDD,契合 §10。备选 B(本 change 即接 live HTTP + 临时 env::var 读 key)经用户确认否决。
- **D4 定义 `Message` / `ToolCall` 数据类型,但不定义 Tool 系统。** `Message`(§5.5)是归一化目标契约,`ToolCall{id,name,arguments:Value}`(§5.1)是其字段所需,故本 change 定义其*数据形状*;但 `Tool` trait / registry / 执行 / 权限一概不做。`provider::ModelRequest` 持 `Vec<Message>`、`agent::Message` 持 `Vec<ToolCall>`(provider 定义)构成单 crate 内的循环模块引用——Rust 合法,接受;若日后拆 crate 再将 `Message`/`ToolCall` 下沉到独立 types 模块。
- **D5(偏离 §5.1,显式标注)`ModelRequest` 省略 `tools` 字段。** §5.1 列了 `tools: Vec<ToolSchema>`,但本 change 无工具系统、无 `ToolSchema` 类型,且无生产者/消费者。按权威次序(code/simplicity 此处优于 plan)省略,待工具 change 补回。备选:占位 `Vec<serde_json::Value>`(弃:投机字段)。
- **D6 归一化目标为非流式响应体;`DeltaSink` 仍定义。** SSE 累积(§5.1 差异表第 4 项)属传输,随 D3 延后。`DeltaSink` 是 §5.1 provider 契约的一部分,成本低且让 stdout 路径真实可跑——由 `MockProvider` 将脚本回复文本作为增量经 sink 吐出来验证。
- **D7 `function.arguments` 字符串解析。** OpenAI 的 tool-call `arguments` 是 JSON 编码的*字符串*;归一化 MUST 将其 parse 为 `serde_json::Value`,使内核见结构化参数;非法 JSON → `ProviderError`。
- **D8 异步运行时。** `tokio`(features 仅 `rt-multi-thread`、`macros`),`#[tokio::main]`;`Provider` 用 `#[async_trait]`(native AFIT 暂不支持 dyn,§5.1)。`sync`/`process`/`time` 等 feature 留到 loop/工具/传输 change。
- **D9 `ProviderError` 本 change 两个变体:`Transport(String)`(§9 已列;含 Mock 脚本耗尽的 fail-safe)+ `Decode(String)`(§9 未列,为归一化解析失败语义显式增补)。** §9 的 `Auth` / `RateLimited` / `Timeout` 要在有真实调用时才有构造点,随传输 change 引入,避免现在堆无构造点的变体。注:`Decode` 是对 §9 的*增补*而非子集,按权威次序在此显式标注偏离(此前「§9 最小集」措辞不准,已订正)。
- **D10 `main` 是薄胶水,不走 red-green。** 核心逻辑在 `run_single_turn(provider, prompt, sink) -> Result<String, ProviderError>`(IO 无关,单测覆盖);`main` 读 prompt(`std::env::args` 拼接,无 prompt 则读一行 stdin;不引 clap)、装配 `MockProvider` + `StdoutSink`、调用 `run_single_turn`,由 `cargo run` 冒烟验证。**可见输出由 `StdoutSink` 在 `complete` 期间流式完成;`run_single_turn` 的返回值仅供测试断言,`main` 不重复打印(至多补一个结尾换行),避免整段回复打两遍。** `main` 错误用 `Result<(), ProviderError>`(无新依赖);§9 的 `anyhow` 留装配层成长时再引。与 CLAUDE.md「TUI 外壳事后回归」同理。
- **D11 edition 2021。** 保守,本 change 代码无需 2024 特性。
- **D12 `FinishReason` 变体与未知值兜底。** 定义 `Stop` / `Length` / `ToolCalls`,映射 OpenAI `stop` / `length` / `tool_calls`;其余值(如 `content_filter`)与缺失 / `null` → `Other(String)`(保留原始串)。保证 forward-compat,解析不因未知 `finish_reason` 失败。
- **D13 `Provider` / `DeltaSink` 契约严格采 §5.1:`&self` + `Send + Sync` + interior mutability。** `Provider::complete(&self, req, sink: &dyn DeltaSink)` 与 `DeltaSink::on_text(&self, delta)` 允许同一 provider/sink 以共享引用进入后续并发 Agent Loop;有状态实现必须自行使用 interior mutability。`MockProvider` 因此用 `AtomicUsize` 管脚本 cursor、`Mutex<Vec<ModelRequest>>` 记录请求,测试读取返回 `MutexGuard` 而不为 `ModelRequest` 增加 `Clone`。备选:`&mut self` / `&mut dyn DeltaSink`(弃:偏离 §5.1,会把 provider/sink 独占借用泄漏到核心契约,不利于后续并发与共享装配)。此项补正此前实现阶段未记录的静默偏离。

## Risks / Trade-offs

- **[契约过早固化]** 首 change 定的 `Message`/`Provider`/`DeltaSink` 形状将被大量下游依赖 → 缓解:严格按 §5.1/§5.5 钉形;新 trait 实现期设 TDD 停点人工确认;行为锁进 OpenSpec spec,后续以 delta 演进。
- **[归一化无真实 endpoint 校验]** Option A 仅以 JSON fixture 验证,可能与真实 wire 细节有偏差 → 缓解:fixture 取自 OpenAI 官方响应样例;真实校验在传输 change 补 live smoke。
- **[循环模块引用]** provider ↔ agent 类型互引 → 缓解:单 crate 内合法;拆 crate 时将 `Message`/`ToolCall` 下沉独立 types 模块。
- **[偏离 §5.1:省略 tools 字段]** → 缓解:已在 D5 显式标注,工具 change 补回;当前无消费者,省略无行为损失。

## Migration Plan

Greenfield,无既有系统;无数据/接口迁移。回滚 = revert 本 change 的提交。

## Open Questions

- 传输 change 是否随凭据链一并引入 `secrecy::SecretString`(§5.6)?留待下个 propose。
- `tokio` flavor 最终取舍(multi-thread vs current-thread)待 loop/TUI change 定;本 change 不敏感。
