## Context

本 change 实现技术方案 §12 第 2 步的「Loop 核心」半(2 拆方案已确认),建立在 change 1 已落地代码之上。已读真实实现并按权威次序(code > spec)对齐如下硬约束:

- `Provider::complete(&self, req: ModelRequest, sink)` 取 `req` **by value**;
- `Message` / `ModelRequest` 当前**未** derive `Clone`;`ModelRequest` 无 `tools` 字段(change 1 design D5);
- `ProviderError` 仅 `Transport` / `Decode`;`MockProvider` 已支持脚本化 + `recorded_requests()`,可直接驱动 Loop 测试。

设计依据:§6(Agent Loop)、§5.3(Tool)、§5.4(权限门)、§5.5(history / 6 事件)、§9(可恢复 vs 致命)、§10(测试范围 1/2/3)。属 CLAUDE.md 强制 TDD。

## Goals / Non-Goals

**Goals:**
- Agent Loop 编排 + 终止条件 + `max_iterations` 守卫;6 类事件入 history。
- `Tool` / `ToolRegistry` / `ToolOutcome` / `ToolContext` / `PermissionLevel` 抽象。
- 可注入的权限门 seam;拒绝映射为 is_error `ToolResult`。
- 工具 schema 下发(补 D5):`ModelRequest.tools` + OpenAI `tools` 序列化。
- 全部 Mock Provider + in-test mock Tool + 注入 decider 驱动 TDD,覆盖 §10 范围 1/2/3。

**Non-Goals(留后续):**
- 实体工具(read/list/glob/grep/write/edit/shell)、tempdir 测试、输出截断行为(change B)。
- live 传输 / 超时重试 / 凭据链 / Anthropic / TUI / 配置分层。
- `Session` 持久化结构(§13 1.2);`main` 改接 Loop 与 stdin y/n decider(change B)。

## Decisions

- **DA1 Loop 落 `agent/mod.rs`(§4「Agent Loop 主控」)。** `loop` 是 Rust 关键字,不能作模块名。以 `Agent` 结构(持 `Box<dyn Provider>` + `ToolRegistry` + `Box<dyn PermissionDecider>` + `model` + `max_iterations`)+ `async fn run(&self, history: &mut Vec<Message>, ctx: &ToolContext, sink: &dyn DeltaSink) -> Result<String, AgentError>` 承载。理由:即 §13「subagent = Session+Registry+Provider 构造的单元」seam;`&mut history` 让测试直接断言 history,对 1.2 持久化友好。备选:多参数自由函数(弃:参数多、不成单元)。`run_single_turn`(conversation)保留不动。
- **DA2 `Message` 加 `Clone`。** `complete` 取 `req` by value,而 Loop 跨轮复用 history → 每轮 `history.clone()` 构造新 `ModelRequest`。备选:改 `complete` 取 `&[Message]` / `&ModelRequest`(弃:破坏 change 1 既定 Provider/wire/mock 契约,远比加一个 derive 侵入)。
- **DA3 `AgentError`(`thiserror`,`error.rs`):`Provider(#[from] ProviderError)`、`MaxIterations{limit}`。** `run` 返回 `Result<String, AgentError>`(Ok = 最终回复)。§9:`max_iterations`、provider 错误为致命;工具失败 / 权限拒绝 / 未知工具为可恢复(is_error `ToolResult`,不进 `AgentError`)。本 change 无重试 → provider 错误一律致命。
- **DA4 权限门(`permission/mod.rs`):`PermissionDecider`(async,dyn 安全)+ `PermissionDecision{Allow,Deny}` + `gate(call, tool, decider) -> PermissionDecision`。** `ReadOnly` → `Allow` 不询问;否则 `decider.decide().await`。`decide` 取 async 是为兼容后续 UI/oneshot 实现;本 change 仅提供测试 decider(AllowAll / DenyAll)。不接 §3 oneshot/UI channel。拒绝由 Loop 注入 is_error `ToolResult`。
- **DA5 补 D5:`ToolSchema` + `ModelRequest.tools`。** `provider/mod.rs` 加 `ToolSchema{name, description, parameters: Value}` 与 `ModelRequest.tools: Vec<ToolSchema>`;`wire::serialize_request` 在 `!tools.is_empty()` 时输出 OpenAI `tools` 数组;`ToolRegistry::schemas()` 由各 `Tool` 的 `name`/`description`/`schema()` 组装。`ToolSchema` 置于 `provider`(属请求契约;`tool` 单向依赖 `provider`,无环)。所有既有 `ModelRequest` 字面量补 `tools: Vec::new()`;无工具时序列化与 change 1 完全一致(无 `tools` 键)。
- **DA6 工具分发顺序执行。** `run` 对每个 tool_call:`registry.get(name)` → None 则 is_error `ToolResult`(未知工具)续;Some 则 `gate` → `Allow` 时 `tool.execute(call.arguments.clone(), ctx).await` → `ToolResult`(content 与 is_error 跟随 `ToolOutcome`);`Deny` 则 is_error `ToolResult`。同轮多个 tool_calls 顺序执行(§6【决策】,并行留 1.5)。
- **DA7 `Session` 结构延后(§13 1.2)。** 本 change Loop 直接 operate on `Vec<Message>` history。
- **DA8 零新依赖。** `HashMap` / `PathBuf` 用 std;async 用既有 `tokio` / `async-trait`。
- **DA9 实体工具 / tempdir / 截断 / main 改接 Loop / stdin decider → change B。** 本 change 测试用 in-test mock `Tool`(一个 `ReadOnly`、一个 `RequiresConfirmation`、一个返回 is_error)+ 注入 decider。
- **DA10 `ToolContext{cwd, max_output_bytes}` 与 `ToolOutcome.truncated` 现在定义,但仅实体工具(change B)消费。** mock tool 可忽略 ctx;截断行为属 change B。

- **DA11 `impl Provider for Arc<T>`(`provider/mod.rs`)。** 支持共享 provider(§3 两-task 模型 / §13 subagent 复用同一 provider);依赖 change 1 把 `complete` 修为 `&self`(Arc 可转发)。本 change 仅测试用到(`Arc::new(mock).clone()` 既塞 `Agent` 又留查 `recorded_requests()`),bin build 暂为 dead_code,待 `main` / 装配接 Loop 后消费。备选:`#[cfg(test)]`(弃:它是面向产品的共享工具,非测试专属)。审查补记,避免又一处未记录的契约面新增。

## Risks / Trade-offs

- **[complete by-value 致每轮 clone history]** → `Message: Clone`;1.0 history 小,成本可接受;若后续 perf 敏感再议借用式 trait。
- **[4 capability 行为重叠(loop/tool/permission)]** → 分工:`agent-loop` 管编排与终止、`tool-system` 管抽象与 schema、`permission-gate` 管门与拒绝映射;拒绝行为单列 `permission-gate`,不在 `agent-loop` 重复测。
- **[ModelRequest 加字段波及 change 1 字面量]** → 机械补 `tools: Vec::new()`;既有 wire 测试输出不变。
- **[Agent 结构 vs 2.0 subagent]** → 即 §13 的 subagent seam,保持最小,不预加 context 预算 / 受限 registry(那是 2.0)。

## Migration Plan

纯加法 + 既有 `ModelRequest` 字面量机械更新;无数据迁移。回滚 = revert 本 change 提交(含撤回 `ModelRequest.tools` 字段)。

## Open Questions

- `run` 是否最终也返回完整 history(而非仅 text)?本 change 取 `&mut history` + 返回 text 已够测试与后续复用;留观。
- `main` 改由 Loop 驱动(空 registry 即退化单轮)的时点 → 定在 change B(有实体工具后才有意义)。
