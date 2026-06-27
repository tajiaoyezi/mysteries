## Why

change 1 已立起 Provider 抽象与单轮对话(`provider-abstraction` / `conversation` 已在主 specs)。但 Agent 还不能多轮编排、不能调用工具、无权限控制——即需求核心的 Agent Loop。本 change(技术方案 §12 第 2 步)补上 headless 内核的心脏:多轮 Loop + 工具系统抽象 + 权限门,全部 Mock 驱动、可充分测试。按已确认的 **2 拆方案**,本 change 只做「Loop 核心 + 抽象」,7 个实体工具留下个 change。

## What Changes

- 新增 **Agent Loop**(§6,`agent/mod.rs` 的 `Agent` 结构 + `run`):每轮以完整 history 请求 provider → Assistant 入 history → 无 `tool_calls` 即终止 / 有则逐个经权限门执行、`ToolResult` 回填后再请求。终止条件:无 `tool_calls`(最终回复)、`max_iterations` 触顶(fatal)、致命错误(fatal)。
- 新增 **工具系统抽象**(§5.3):`Tool` trait、`ToolRegistry`、`ToolOutcome`、`ToolContext`、`PermissionLevel`。**不含实体工具**(read/list/glob/grep/write/edit/shell 留下个 change),测试用 in-test mock `Tool`。
- 新增 **权限门**(§5.4):`ReadOnly` 直跑;`RequiresConfirmation` 经**可注入的 `PermissionDecider` seam**(async)决策;**不接 §3 oneshot / UI channel**(留 TUI change),测试注入 Allow/Deny;拒绝 → is_error `ToolResult` 入 history,循环继续(不静默跳过)。
- 新增 `AgentError`(`thiserror`:`Provider` / `MaxIterations`);Loop 区分**可恢复**(工具失败、权限拒绝、未知工具 → is_error `ToolResult` 续)与**致命**(provider 错误、`max_iterations` → 终止)。
- 6 类事件全部映射进 history(§5.5):User / Assistant.text / Assistant.tool_calls / ToolResult / 拒绝→ToolResult{is_error} / 错误→ToolResult{is_error}。
- **补回 change 1 按 D5 省略的 `ModelRequest.tools`**:新增 `ToolSchema{name,description,parameters}` 与 `ModelRequest.tools` 字段;OpenAI wire 在有工具时序列化 `tools` 数组(registry 提供 schema)。
- `Message` 加 `Clone` derive(Loop 每轮需克隆 history 喂进 by-value 的 `complete`,见 design)。

**明确不含**(留后续):7 个实体工具 + tempdir 测试 + 输出截断行为(change B);live HTTP/SSE/超时重试/凭据链、Anthropic、TUI、配置分层;`Session` 持久化结构(§13 1.2);`main` 改接 Loop 与 stdin y/n decider(随实体工具到 change B 才有意义,本 change `main` 仍走单轮)。

**零新依赖**:Loop/工具/权限仅用 std(`HashMap`/`PathBuf`)+ 既有 `tokio`/`async-trait`/`serde_json`。本 change 不触及 UI。

## Capabilities

### New Capabilities
- `agent-loop`: 多轮编排——请求/回填/再请求、终止条件、`max_iterations` 守卫、6 类事件入 history、可恢复 vs 致命。
- `tool-system`: 工具抽象——`Tool` trait / `ToolRegistry` / `ToolOutcome` / `ToolContext` / `PermissionLevel`,registry 产出 schema 供下发;不含实体工具。
- `permission-gate`: 可注入权限门——`ReadOnly` 放行、`RequiresConfirmation` 经 `PermissionDecider` seam、拒绝入 is_error history。

### Modified Capabilities
- `provider-abstraction`: ADDED——`ModelRequest` 携带工具定义,OpenAI 请求序列化 `tools` 数组(补回 change 1 D5 省略项;不改既有消息序列化行为)。

## Impact

- **新增代码**:`src/tool/mod.rs`、`src/permission/mod.rs`;`src/agent/mod.rs` 加 `Agent` + `run`;`src/provider/mod.rs` 加 `ToolSchema` + `ModelRequest.tools`;`src/error.rs` 加 `AgentError`;`src/provider/wire.rs` 扩展 tools 序列化;`src/agent/message.rs` 给 `Message` 加 `Clone`;`src/main.rs` 加 `mod tool; mod permission;`。
- **改动既有**:所有 `ModelRequest` 字面量补 `tools: Vec::new()`(wire/mock/provider 测试、`run_single_turn`、`main`);现有 wire 测试输出不变(无工具不输出 `tools` 键)。
- **前置依赖**:本 change 实现依赖 change 1 代码(已在仓库)。
- **测试**:强制 TDD,§10 范围 1/2/3(主循环、工具调用与结果回传、权限确认与拒绝)由 Mock Provider + in-test mock Tools + 注入 decider 覆盖(**无 tempdir**——实体工具与 tempdir 属 change B);新 trait(`Tool`、`PermissionDecider`)实现期设停点(CLAUDE.md 折中档)。
- **零新依赖**。
