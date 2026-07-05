# permission-gate Delta

## MODIFIED Requirements

### Requirement: 权限模式 PermissionMode

系统 SHALL 定义 `PermissionMode {Normal, AcceptEdits, Yolo, Plan}` 与纯函数策略 `auto_allows(mode, level) -> bool`,语义为:`Normal` 对任何非 `ReadOnly` 均**不**自动放行(需询问);`AcceptEdits` 自动放行 `Edit`、**不**自动放行 `Execute`;`Yolo` 自动放行 `Edit` 与 `Execute`;**`Plan` 对任何非 `ReadOnly` 均不自动放行**——Plan 的只读约束主要由 tool-system 的 schema-omit(非只读工具不下发)与 agent-loop 的纵深拒承接,见对应能力。`PermissionDecider` 的具体实现 MUST 在发起 UI 询问前查 `auto_allows`,命中即返回 `Allow` 且 MUST NOT 触发 UI 往返(不阻塞、不弹框)。`ReadOnly` 仍由门直接放行,与模式无关(故 **`Plan` 期只读研究工具照常自动放行**)。模式为**运行时可变**的共享状态,默认 `Normal`,不跨进程持久化;运行时经纯函数 `cycle_permission_mode` 轮转 `Normal→AcceptEdits→Yolo→Plan→Normal`。`auto_allows` 与 `cycle_permission_mode` 为纯函数,headless 强制 TDD。

#### Scenario: Normal 模式所有改动类均询问

- **WHEN** `auto_allows(Normal, Edit)` 与 `auto_allows(Normal, Execute)`
- **THEN** 均为 `false`(需询问)

#### Scenario: AcceptEdits 放行编辑、仍问执行

- **WHEN** `auto_allows(AcceptEdits, Edit)`
- **THEN** 为 `true`(自动放行)
- **WHEN** `auto_allows(AcceptEdits, Execute)`
- **THEN** 为 `false`(需询问)

#### Scenario: Yolo 放行一切改动

- **WHEN** `auto_allows(Yolo, Edit)` 与 `auto_allows(Yolo, Execute)`
- **THEN** 均为 `true`(自动放行)

#### Scenario: Plan 模式非只读不自动放行、只读照常放行

- **WHEN** `auto_allows(Plan, Edit)` 与 `auto_allows(Plan, Execute)`
- **THEN** 均为 `false`;而 `ReadOnly` 工具仍由门直接放行(Plan 期研究工具照常自动跑)

#### Scenario: decider 命中自动放行不走 UI 往返

- **WHEN** 当前模式 `Yolo`,对一个 `Execute` 工具经 decider 决策
- **THEN** decider 返回 `Allow`,且不发起 oneshot / channel 询问

#### Scenario: cycle 轮转纳入 Plan

- **WHEN** 从 `Yolo` 连续 `cycle_permission_mode`
- **THEN** 依次 `Yolo→Plan→Normal`(Plan 在 Yolo 之后、环回 Normal)
