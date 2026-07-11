## Why

Provider 一次返回多个互不依赖的本地读取 / 搜索 `tool_calls` 时，Agent Loop 当前仍逐个等待，仓库调研延迟随调用数线性累加；同时，`PermissionLevel::ReadOnly` 还包含 `ask_user`、`submit_plan`、`update_plan` 等交互或进程内状态工具，不能被当作并发安全的替代分类。Network 权限边界现已归档，可以按路线图 1.5 引入独立、默认保守的工具并发契约，为后续 subagent 调度提供有界执行、顺序发布与错误隔离先例；本 change 的取消收口仅覆盖 TUI turn，不构成通用 Agent / subagent cancellation API。

## What Changes

- `Tool` 新增与 `PermissionLevel` 正交的 `ToolConcurrency::{Exclusive, ParallelSafe}` 元数据；默认 `Exclusive`，未显式 opt-in 的新工具保持串行。
- 首版仅将 `list_dir`、`read_file`、`glob`、`grep` 标为 `ParallelSafe`；`web_fetch` / `web_search`、Edit、Execute、Plan 与用户交互工具全部保持 `Exclusive`，权限门与四种 `PermissionMode` 语义不变。
- Agent Loop 将同一模型回复中的最大连续 `ParallelSafe` 调用段组成 work-conserving 有界批次，每个 Agent 最多同时执行 4 个；`Exclusive`、未知工具及其他不可并行路径形成顺序屏障，后续调用不得越过。物理完成按空槽持续补位，不被较早慢调用阻塞。
- 并行批次先按模型顺序发出 started 观测，再以原 index 收集无序完成结果；只有连续 ready 前缀能发布 finished 与 `ToolResult`，因此公开顺序仍与原始 `tool_calls` 一致。单项 `ToolOutcome.is_error` 不取消同批其他项，下一轮 Provider 请求只在整批结果完整入 history 后发起。
- 四个本地读取工具把同步 `std::fs` / `ignore::WalkBuilder` 工作迁入 `spawn_blocking`，并共用进程级 `Semaphore(4)`；permit 移入 blocking closure、直到真实工作结束才释放，使 Interrupt 后旧 closure 与新 turn 合计仍不超过 4。不新增 crate。
- TUI 允许同一时间存在多张 Running C5 工具卡，并为并行批次增加聚合活动状态：count 表示整个已调度批次；count≤4 显示“并行执行 N 个工具…”，count>4 显示“处理 N 个工具（最多并行 4）…”。Running 表示已纳入批次、可能正在执行或等待空槽；单工具 C5 与 `ExecutingTool(name)` 呈现保持不变。
- TUI Interrupt 将所有 Running 工具卡收口为“已中断”，并在保存当前 working history 前按 tool-call occurrence / 顺序多重集为未配对调用补 is_error interrupted `ToolResult`；不得假设 call id 在同一 turn 唯一。丢弃已取消批次的迟到结果，不发送 trailing finished / Idle 事件。
- 两处 session load site（`--continue` 启动加载；`--resume` picker 选中后的 runtime hot-swap）在激活持久化 session 前做双重收口：旧 Running C5 卡规范化为 Error / “上次会话已中断”；loaded history 中每组未配对 `Assistant.tool_calls` 按 occurrence / FIFO 补 is_error interrupted `ToolResult`。这样既避免复用 `call_id` 时 finished 误回填历史卡，也保证恢复后的首次 Provider 请求无 dangling call；不改变 `SessionStore::load` 的 round-trip 或 session JSONL wire。
- 修订技术方案路线图中“按 ReadOnly 推断并发”的旧提示，改为独立 concurrency metadata；同步 README / CHANGELOG 的用户可见能力说明。
- 明确不在本 change：Network 并行授权、Edit / Execute 资源冲突图、可配置并发数、跨 `Exclusive` 重排、Provider 请求并行、通用 Agent cancellation、MCP 与 subagent 实装。

## Capabilities

### New Capabilities

- 无；并发分类属于既有工具抽象，并发调度属于既有 Agent Loop 编排职责。

### Modified Capabilities

- `tool-system`：为 `Tool` 增加独立、默认 `Exclusive` 的并发策略元数据，并明确其不得从权限级别推断。
- `builtin-tools`：锁定 12 个内置工具的并发分类，并让四个本地读取工具以非阻塞 Tokio worker 的方式执行同步文件工作。
- `agent-loop`：把逐个工具处理改为“连续安全批次 + 独占屏障”的有界调度，同时锁定结果顺序、错误隔离与 observer 事件契约。
- `tui-shell`：承载多个同时 Running 的 C5 工具卡、并行批次 C10 活动状态及 Interrupt 后的卡片 / history 收口。
- `session-persistence`：在 `SessionStore::load` 保持原始 round-trip 的前提下，为 `--continue` 与 `--resume` hot-swap 增加激活前的旧中断残留 normalization。

## Impact

- **主要代码**：`src/tool/mod.rs`、`src/tool/fs.rs`、`src/agent/mod.rs`、`src/tui/channel.rs`、`src/tui/app.rs`、`src/tui/mod.rs`、`src/tui/render.rs` 及其测试 / 快照。
- **UI / 设计规范**：port `设计规范/03-组件清单.md` C5 既有 per-call `running|done|error` 结构，使多卡可同时处于 Running；adapt `设计规范/02-布局与交互.md` 状态机与 C10 活动状态行，在不新增布局区域的前提下为多调用显示聚合数量；drop 新面板、并行动画与每个工具独立 spinner，避免增加视觉噪声。实现仍以 `TestBackend + insta` 的 Midnight / Daylight 带色快照为视觉事实源。
- **配置 / 依赖**：使用现有 `tokio` 与 `futures-util`；不新增 crate，不新增 config 字段，不改变 CLI flags、权限 prompt、session JSONL 或 ToolCard 持久化格式。
- **兼容性**：`Tool::concurrency()` 有默认实现，现有自定义 Tool 源码保持可编译并默认串行；`PermissionLevel`、Network preview、mode matrix 与 schema 顺序不变。新增 `AgentStatus` / TUI `Phase` 并行变体会由编译器暴露内部穷尽匹配接线点；旧 session 无需迁移文件，加载时只在内存中收口遗留 Running 卡与未配对 tool-call occurrence。
- **验证**：headless 调度与中断 history 契约按 TDD 先 RED 后 GREEN；TUI 外壳事后补 `TestBackend + insta` 回归；最终运行完整 Rust build/test/clippy 与 strict OpenSpec validation。
