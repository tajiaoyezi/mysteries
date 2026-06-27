## ADDED Requirements

### Requirement: 命令块渲染(C8 / C9 / notice)

`render` SHALL 渲染命令产出的 transcript 块(`设计规范/03`):C8 帮助块(两列 `cmd` + `desc`,7 命令)、C9 快照块(`provider · model · iter X/maxIter · N msgs · cwd · tools: 7`)、notice 块(info / 占位提示,`info.fg` / 框)。带色,复用 `Theme` + `buffer_to_styled`。

#### Scenario: 帮助块与快照块带色快照

- **WHEN** transcript 含一个 C8 帮助块 / 一个 C9 快照块时渲染
- **THEN** 各自 `insta` 带色快照与锁定一致(C8 两列对齐 7 命令;C9 含 provider/model/iter/msgs/cwd/tools 字段)

### Requirement: 状态行常驻 meta

状态行右侧 SHALL 常驻显示 `provider · model · iter X/maxIter · N msgs · cwd`(`设计规范/02` C10),与左侧 phase 并存。`iter` 由 UI 统计当前轮的 `StatusChanged(CallingModel)` 次数得到(新轮 / `TurnComplete` 重置),`msgs` = transcript 块数,其余取 session 快照(`/model` 切换后 model 同步更新)。

#### Scenario: 状态行 meta 快照

- **WHEN** 给定 session 快照(provider/model/maxIter/cwd)与若干 transcript 块渲染
- **THEN** 状态行右侧带色快照含 `provider · model · iter X/maxIter · N msgs · cwd`,与锁定一致

### Requirement: 工具卡 C5 exit foot

`ToolCard` SHALL 携 `exit: Option<i32>`(由 `ToolCallFinished` 的 `outcome.exit` 填);`render` SHALL 仅在 `exit` 为 `Some` 时渲染 C5 脚 `exit {code}`(非 0 用 `error.fg`),`None` 时**不渲染 foot**(保证既有非进程类工具卡快照零回归)。

#### Scenario: run_shell 卡含 exit foot,其余不含

- **WHEN** 渲染一个 `exit = Some(0)` 的工具卡与一个 `exit = None` 的工具卡
- **THEN** 前者带色快照含 `exit 0` 脚;后者无 foot(与既有工具卡快照一致)

### Requirement: UserInput::SetModel 变体

`tui` 的 `UserInput` SHALL 增 `SetModel(String)` 变体;`run_agent_task` 收到时 MUST 对 idle 的 agent 调 `Agent::set_model`,不影响进行中的轮。

#### Scenario: SetModel 改后续轮 model

- **WHEN** agent-task 在两轮之间收到 `UserInput::SetModel("m2")`
- **THEN** 后续 `Prompt` 轮的 `ModelRequest.model` 为 `"m2"`
