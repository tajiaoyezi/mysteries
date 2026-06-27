## ADDED Requirements

### Requirement: 结构化事件经 ChannelObserver 上送

`tui` 的 `AgentEvent` SHALL 扩展 `ToolCallStarted{id, name, args, readonly}` / `ToolCallFinished{id, outcome}` / `StatusChanged(AgentStatus)`。系统 SHALL 提供 `ChannelObserver`(impl `AgentObserver`),把观测回调 forward 成对应 `AgentEvent` 经 `mpsc::UnboundedSender` 上送(mirror 既有 `ChannelSink` / `ChannelDecider`);`run_agent_task` MUST 改调 `Agent::run_observed(.., &ChannelSink, &ChannelObserver)`,使文本与结构化事件经同一 channel 流回 UI。

#### Scenario: 工具轮的结构化事件流(Mock · 无终端)

- **WHEN** 以 Mock 脚本(含一个工具的 tool_call)驱动 `run_agent_task`,对权限请求回送 `Allow`
- **THEN** channel 依次收到 `StatusChanged(CallingModel)` → `ToolCallStarted` → `ToolCallFinished` → 后续文本 → `TurnComplete`,全程无终端

### Requirement: 工具卡 C5 渲染

`AppState` SHALL 据 `ToolCallStarted` / `ToolCallFinished` 维护工具卡块;`render` SHALL 按 `设计规范/03` C5 渲染:头(状态 glyph `running`→占位 / `done`→`✓` / `error`→`✗` + 工具名 + args;只读工具带 `只读 · 自动运行` 徽章)、体(`output` 行;截断时 `⋯ +N 行已截断`)、脚(`exit {code}`)。本 change 为**结构态**(最小色,主题留 cut2b;`running` 用静态字符,spinner 留 cut2b)。

#### Scenario: 工具卡三态结构快照

- **WHEN** 分别以 running / done / error 态的工具卡渲染到 `TestBackend`
- **THEN** `insta` 快照含 C5 结构(glyph + 名 + args + 只读徽章 + output + exit + 截断标记),且与锁定快照一致

### Requirement: 全 phase 状态行 C10

状态行 SHALL 据 `StatusChanged` 显示完整 phase(`设计规范/02` 状态机):`Idle`→`◇ 就绪`、`CallingModel`→`调用模型…`、`ExecutingTool(name)`→`执行 {name}…`、`WaitingForPermission`→`▲ 等待授权…`(替换 cut1 的粗 phase)。`AppState` 的 phase 状态 MUST 可单测,渲染 MUST 可 `insta` 快照。

#### Scenario: phase 随事件更新(状态可测)

- **WHEN** `AppState.apply(StatusChanged(ExecutingTool("write_file")))`
- **THEN** 其 phase 为 `ExecutingTool("write_file")`,后续渲染状态行左侧显示 `执行 write_file…`

#### Scenario: 各 phase 状态行快照

- **WHEN** 分别以 `Idle` / `CallingModel` / `ExecutingTool(x)` / `WaitingForPermission` 渲染状态行
- **THEN** 各自 `insta` 快照与锁定一致(glyph + label 正确)
