## Why

仓库当前无任何 Rust 代码,Agent 还无法与任何 LLM 通话。1.0 的第一步必须立起 headless 内核最底层的 seam:一个 IO 无关、可 Mock、协议归一化的 `Provider` 抽象,并用一条最小的「输入 → 模型 → stdout」单轮链路证明这层抽象能跑通。这是技术方案 §12 第 1 步,后续 Agent Loop、工具、权限、TUI 全部挂在它之上(§13 扩展缝)。

## What Changes

- 新建 cargo 二进制 crate(repo 首次引入 Rust 代码),建立 `provider` / `agent::message` / `error` 模块骨架——仅本 change 涉及的部分,不预建空模块。
- 引入 `Provider` trait(`#[async_trait]`,dyn 安全)与流式出口 `DeltaSink` trait;Provider 不感知 UI channel(§5.1【决策】)。
- 定义内核规范类型:`Message`(System/User/Assistant/ToolResult,§5.5)、`ModelRequest`、`ModelResponse`、`FinishReason`、`ToolCall`(§5.1)。
- 实现 **OpenAI 兼容协议归一化**(`provider::wire`):`Message[]` → OpenAI 请求体序列化;OpenAI 非流式响应体 → `ModelResponse` 解析。覆盖 §5.1 归一化差异表中 OpenAI 侧三项(`tool_calls` 表示、tool 结果回传 `role:"tool"`、system prompt 为 `role:"system"` 消息)。
- 实现 `MockProvider`(脚本化 `Vec<ModelResponse>` + 记录收到的 `ModelRequest`,§10),作为测试基础设施与 stdout demo 的驱动。
- 打通**纯 stdout 单轮对话**:读取一条 prompt → 组装 `ModelRequest`(System + User)→ `provider.complete` → 经 `DeltaSink` 输出到 stdout。本 change 由 `MockProvider` 驱动(离线、确定性)。
- 类型化错误 `ProviderError`(`thiserror`:`Transport`(§9)+ `Decode`(§9 未列,归一化解析失败语义增补))。

**明确不含**(留后续 change,按用户范围):Agent Loop 多轮、工具系统、权限门、TUI、配置分层、凭据链、Anthropic provider。

**live HTTP / 凭据(已与用户确认取 Option A)**:本 change **不**引入 reqwest / SSE 实传输与任何 API key 读取。OpenAI 的*归一化*(wire serialize/parse)在此交付并全程 TDD;OpenAI 的*实传输*(reqwest + SSE 累积,§5.2)与凭据链(§5.6)天然耦合,一并留到下一个 change。因此 §5.1 归一化差异表第 4 项「流式工具增量」属传输层,本 change 不实现。

本 change 不触及 UI,故不涉及 `设计规范/` 引用。

## Capabilities

### New Capabilities
- `provider-abstraction`: IO 无关、可 Mock 的 LLM Provider 接入层——`Provider` / `DeltaSink` trait、内核规范消息/请求/响应类型、OpenAI 协议归一化、`MockProvider`。
- `conversation`: 单轮对话链路——一条 prompt 经规范类型与 Provider 抽象产出一次模型回复并输出到 stdout(1.0 仅单轮;多轮 Agent Loop 留后续 change)。

### Modified Capabilities
<!-- 无。这是首个 change,openspec/specs/ 为空,不存在既有 capability 的 requirement 变更。 -->

## Impact

- **新增代码**:`Cargo.toml`、`src/main.rs`、`src/error.rs`、`src/provider/{mod,wire,mock}.rs`、`src/agent/{mod,message}.rs`。
- **新增依赖**(均见 §11;本 change 仅用到这 5 个):`tokio`(`rt-multi-thread`、`macros`)、`async-trait`、`serde`(`derive`)、`serde_json`、`thiserror`。无 reqwest / SSE / secrecy / clap / tracing(随后续 change 引入)。
- **构建/测试基线**:`cargo build` 通过;归一化与 Mock 走强制 TDD(§10),`cargo test` 全绿。
- **下游契约**:确立模块边界与 `Message` / `Provider` / `DeltaSink` 形状,后续 Agent Loop、工具、权限、TUI、Anthropic、凭据链均在此之上扩展。
- **风险**:契约一旦被多处依赖,改动成本上升——故首 change 即按 §5.1 / §5.5 钉死类型形状;新 trait 在实现期设 TDD 停点人工确认(CLAUDE.md TDD 折中档)。
