# permission-gate Specification

## Purpose
permission-gate 是工具执行前的集中放行决策点:`ReadOnly` 工具直接放行,`Edit` / `Execute` 工具经可注入的 `PermissionDecider` 取得 `Allow` / `Deny`;拒绝不静默跳过,而是产出 is_error 的 `ToolResult` 入 history,使循环带着被拒上下文继续。设计立场是门与 UI 解耦——decider 为 async 注入 seam,不绑定具体 channel;运行时 `PermissionMode`(`Normal` / `AcceptEdits` / `Yolo`)以纯函数 `auto_allows` 在询问前判定,命中自动放行即不发起 UI 往返。本域只按 tool-system 声明的 `permission_level` 做放行裁决,拒绝之后的 history 与续跑行为由 agent-loop 承接。
## Requirements
### Requirement: 可注入的权限门

系统 SHALL 提供权限门:`ReadOnly` 工具直接放行(不询问);非 `ReadOnly` 工具(`Edit` / `Execute`)经**可注入的 `PermissionDecider` seam**(async)取得 `Allow` / `Deny`。本 change MUST NOT 将门绑定到 §3 的 oneshot / UI channel;测试以注入式 decider 提供决策。门是集中决策点;运行时权限模式策略(见「权限模式 PermissionMode」)由 decider 在询问前裁决,门本身只按 level 派发。

#### Scenario: 只读工具直接放行

- **WHEN** 对一个 `ReadOnly` 工具调用权限门
- **THEN** 返回 `Allow`,且不调用 `PermissionDecider`

#### Scenario: 变更工具经 decider 决策

- **WHEN** 对一个 `Edit` 或 `Execute` 工具调用权限门,并注入一个总是返回 `Allow` 的 decider
- **THEN** `PermissionDecider` 被调用,门返回 `Allow`

### Requirement: 拒绝产出 is_error ToolResult

被拒绝的工具调用 SHALL NOT 执行,且 MUST 产出一条 is_error 的 `ToolResult`(「user denied」类)入 history,循环据此继续(不静默跳过)。

#### Scenario: 拒绝 → denial 入 history 且续跑

- **WHEN** 注入一个总是返回 `Deny` 的 decider,Loop 处理一个 `Edit` / `Execute`(非 `ReadOnly`)的 tool_call
- **THEN** 该工具不被执行,一条 `ToolResult{is_error: true}` 入 history,Loop 继续发起下一轮请求

### Requirement: 权限模式 PermissionMode

系统 SHALL 定义 `PermissionMode {Normal, AcceptEdits, Yolo}` 与纯函数策略 `auto_allows(mode, level) -> bool`,语义为:`Normal` 对任何非 `ReadOnly` 均**不**自动放行(需询问);`AcceptEdits` 自动放行 `Edit`、**不**自动放行 `Execute`;`Yolo` 自动放行 `Edit` 与 `Execute`。`PermissionDecider` 的具体实现 MUST 在发起 UI 询问前查 `auto_allows`,命中即返回 `Allow` 且 MUST NOT 触发 UI 往返(不阻塞、不弹框)。`ReadOnly` 仍由门直接放行,与模式无关。模式为**运行时可变**的共享状态,默认 `Normal`,不跨进程持久化。`auto_allows` 为纯函数,headless 强制 TDD。

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

#### Scenario: decider 命中自动放行不走 UI 往返

- **WHEN** 当前模式 `Yolo`,对一个 `Execute` 工具经 decider 决策
- **THEN** decider 返回 `Allow`,且不发起 oneshot / channel 询问

