## Why

v1.2.0 已有有界并行工具批次，但中断收口仍是 TUI turn 外层 helper，`Agent` 本身没有可复用的运行身份、cancellation、预算或 capability scope；直接在此基础上实现 subagent，会让 child 生命周期、权限上限和中断后的 history 完整性依赖调用方临时拼装。v1.3.0 的只读 subagent MVP 需要先把一次 Agent run 固化为可安全派生、可取消且不能扩权的执行单元。

## What Changes

- 新增 Agent execution scope：每次 run 具有稳定 run identity、可选 parent identity、可传播的 cancellation，以及独立的 iteration / deadline / child-depth 预算。
- scope 派生遵循单调收窄：child 只能缩小父 scope 的工具集合、权限级别与预算，不能恢复父级已禁止的 capability。
- 为 `Agent` 增加显式 scoped run 入口；既有 `run` / `run_observed` 保持为 root scope 兼容入口，不改变现有调用语义。
- 把 cancellation 检查与收口放入 Agent Loop：模型等待、串行工具和并行安全批次均可被统一中断，已开始或尚未执行的 tool call 都产生顺序稳定、occurrence 完整的错误结果，不把 dangling call 留给 TUI 猜测。
- 让 `ToolRegistry` 支持安全共享和按名称构造受限 registry/view；未知、重复或越界请求 fail-closed。
- 在权限门前应用 execution scope capability 上限；`PermissionMode::Yolo`、命令 allowlist 或 decider 的允许决定都不能绕过 scope 禁止。
- 让 observer 事件携带 run identity，使后续 subagent 能区分 parent/child 事件；本 change 只保持现有 TUI 可见行为，不增加 child UI。
- 明确不新增 `delegate_task` / subagent 工具、不创建 child session、不改变 session wire、不实现递归 Agent graph、MCP 或新的 TUI 布局。

## Capabilities

### New Capabilities
- `agent-execution-scope`: 定义 Agent run identity、派生关系、cancellation、预算、capability 单调收窄与确定性收口契约。

### Modified Capabilities
- `agent-loop`: 增加 scoped run 入口，并把 cancellation、顺序稳定的中断结果与 observer run identity 纳入 Loop 契约。
- `tool-system`: 工具注册表支持安全共享和 fail-closed 的受限视图，供派生 execution scope 使用。
- `permission-gate`: execution scope capability 上限先于模式、allowlist 与用户决策生效，任何下游允许路径均不得扩权。

## Impact

- 主要影响 `src/agent/`、`src/tool/mod.rs`、`src/permission/` 及相关 Mock / headless 测试；TUI runtime 只接入新的 root cancellation seam，不改布局、样式或 session 格式。
- 这是 headless 内核与新权限路径，实施必须严格按 RED→GREEN 分步；新接口首次成型的 RED 证据按仓库约定停下等待确认。
- 优先复用现有 `tokio` 同步原语，不预设新增 dependency；若 design 证明必须引入 crate，须先单独说明必要性与替代方案。
- 后续 `add-readonly-subagent` 将依赖本 capability，但不属于本 change。
