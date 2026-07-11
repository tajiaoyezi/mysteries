## MODIFIED Requirements

### Requirement: 可注入的权限门

系统 SHALL 提供权限门:`ReadOnly` 工具直接放行(不询问);`Edit` / `Execute` 经**可注入的 `PermissionDecider` seam**(async)取得 `Allow` / `Deny`。系统 MUST 定义 `PermissionCheck<'a>{call:&'a ToolCall, tool:&'a dyn Tool, network_preview:Option<&'a NetworkPermissionPreview>}`；非 Network 的 preview 恒为 None，Network MUST 由 gate 调一次 Tool preview 后传 Some。`PermissionDecider::decide(check)` 仍返回 `PermissionDecision::{Allow,Deny}`；gate 返回 `PermissionGateOutcome::{Allow,Deny(PermissionDenial)}`，且 `PermissionDenial::{UserDenied,NetworkUnauthorizable(String)}`；下文以 `Deny(UserDenied)` / `Deny(NetworkUnauthorizable(reason))` 简写。gate MUST 在 decider 返回后对 `authorizable=false` clamp 为 NetworkUnauthorizable。本 requirement MUST NOT 绑定 TUI channel；mode / policy 由 decider 裁决，但不得越过 clamp。

#### Scenario: 只读工具直接放行

- **WHEN** 对一个 `ReadOnly` 工具调用权限门
- **THEN** 返回 `Allow`,且不调用 `PermissionDecider`

#### Scenario: 有效 Network / Edit / Execute 经 decider 决策

- **WHEN** 分别对一个提供 `authorizable=true` preview 的 `Network`、一个 `Edit`、一个 `Execute` 工具调用权限门,并注入总是返回 `Allow` 的 decider
- **THEN** Network preview 仅计算一次并以 `PermissionCheck.network_preview=Some` 传给 decider；Edit / Execute 为 None；三次 decider 均被调用,gate outcome 均为 Allow

#### Scenario: decider 拒绝可授权的非 ReadOnly level

- **WHEN** 对一个提供 `authorizable=true` preview 的 `Network` / 任一 `Edit` / `Execute` 注入总是返回 `Deny` 的 decider
- **THEN** gate 返回 `Deny(UserDenied)`,不会退化为 `Allow`

### Requirement: 拒绝产出 is_error ToolResult

被拒绝的 `Network` / `Edit` / `Execute` 工具调用 SHALL NOT 执行,且 MUST 产出一条 is_error `ToolResult` 入 history并续跑。用户 / UI 返回 Deny 时 outcome 为 `UserDenied`、content 保持既有 user denied 语义；preview 不可授权时 outcome 为 `NetworkUnauthorizable(reason)`、content MUST 带 preview denial reason，不得归因为用户拒绝。两种 Network Deny 均发生在 `tool.execute` 前,不得产生 DNS、proxy、TCP、TLS、HTTP 或任何 WebFetcher 调用。

#### Scenario: 拒绝 → denial 入 history 且续跑

- **WHEN** 注入一个总是返回 `Deny` 的 decider,Loop 处理一个 `Network` / `Edit` / `Execute`(非 `ReadOnly`)的 tool_call
- **THEN** 该工具不被执行,一条与 gate denial kind 对应的 `ToolResult{is_error: true}` 入 history,Loop 继续发起下一轮请求

#### Scenario: Network 拒绝保证零网络

- **WHEN** Loop 处理一个 `Network` tool_call,decider 返回 `Deny`,工具注入记录调用次数的 spy `WebFetcher`
- **THEN** spy 调用次数为 0,且没有 DNS / proxy / TCP / TLS / HTTP 活动

### Requirement: 权限模式 PermissionMode

系统 SHALL 定义 `PermissionMode {Normal, AcceptEdits, Yolo, Plan}` 与纯函数策略 `auto_allows(mode, level) -> bool`,语义为:

- `Normal`:不自动放行 `Network` / `Edit` / `Execute`,均需询问；
- `AcceptEdits`:仅自动放行 `Edit`,`Network` / `Execute` 仍需询问；
- `Yolo`:`auto_allows` 对 `Network` / `Edit` / `Execute` 返回 true；Network 仍须先满足 authorizable；
- `Plan`:不自动放行 `Network` / `Edit` / `Execute`;其中 Network 由 agent-loop 保留 schema 并进入 decider,Edit / Execute 由 schema-omit + 纵深拒承接。

`PermissionDecider` 的具体实现 MUST 先处理 gate 传入的 preview：`authorizable=false` 时不得命中 policy / mode Allow；TUI 可发 reject-only 往返，headless 直接 Deny。有效调用才在发起 UI 询问前查 `auto_allows`,命中即返回 `Allow` 且不触发 UI 往返。`ReadOnly` 仍由门直接放行、与模式无关；`auto_allows(_, ReadOnly)` 可保持 false。模式为运行时可变共享状态,默认 `Normal`,不跨进程持久化；`cycle_permission_mode` MUST 轮转 `Normal→AcceptEdits→Yolo→Plan→Normal`。矩阵与轮转均为 headless 纯逻辑,强制 TDD。

#### Scenario: Normal 模式 Network / Edit / Execute 均询问

- **WHEN** `auto_allows(Normal, Network)`、`auto_allows(Normal, Edit)`、`auto_allows(Normal, Execute)`
- **THEN** 均为 `false`

#### Scenario: AcceptEdits 只放行编辑

- **WHEN** 分别计算 `auto_allows(AcceptEdits, Network / Edit / Execute)`
- **THEN** 仅 `Edit` 为 `true`,Network 与 Execute 为 `false`

#### Scenario: Yolo 放行 Network 与改动类

- **WHEN** 分别计算 `auto_allows(Yolo, Network / Edit / Execute)`
- **THEN** 均为 `true`

#### Scenario: Plan 保留 Network 询问语义

- **WHEN** 分别计算 `auto_allows(Plan, Network / Edit / Execute)`
- **THEN** 均为 `false`;ReadOnly 仍由 gate 直接放行,Network 由 decider 询问,Edit / Execute 由 agent-loop 纵深拒

#### Scenario: 有效 preview 在 Yolo 自动放行不走 UI 往返

- **WHEN** 当前模式 `Yolo`,分别对 authorizable Network / Edit / Execute 工具经 decider 决策
- **THEN** decider 返回 `Allow`,且不发起 oneshot / channel 询问；Network 最终仍由 gate 复核 authorizable

#### Scenario: cycle 轮转纳入 Plan

- **WHEN** 从 `Yolo` 连续 `cycle_permission_mode`
- **THEN** 依次 `Yolo→Plan→Normal`(Plan 在 Yolo 之后、环回 Normal)

## ADDED Requirements

### Requirement: Network 权限仅允许本次调用

Network 调用在本 change 中 MUST 不产生 command permission key,不得命中 `allowed_commands`,不得提供或持久化 always-allow。一次 `AllowOnce` 只授权当前一个 `ToolCall`;同一 origin 的后续调用也 MUST 重新决策。异常路径若收到 `PermissionReply::AllowAlways`,MUST 仅允许当前调用,不得写 config、不得记入内存 policy。

#### Scenario: Network 不命中命令 allowlist

- **WHEN** Network 调用的 args 含 `url` 或 `query`,即使其字符串与某条 `allowed_commands` 相同
- **THEN** `permission_key` 仍为 `None`、`PolicyEngine::is_allowed` 为 false

#### Scenario: Network 不提供 always-allow

- **WHEN** Normal / AcceptEdits / Plan 下对 authorizable Network 调用发起权限询问
- **THEN** 请求的 `allow_always_key` 为 `None`,UI 只提供 AllowOnce / Deny

#### Scenario: 意外 AllowAlways 不跨调用生效

- **WHEN** keyless Network 请求异常收到 `AllowAlways`,随后又出现相同 origin 的第二个 Network ToolCall
- **THEN** 第一次只映射为当前 `Allow`、不落盘不记忆；第二次仍须重新决策

### Requirement: Network authorizability 在所有 mode 下 fail-closed

Network 的 authorizability 检查 MUST 先于 command policy、mode 自动放行与有效 Allow。gate MUST 将同一 preview 传给 decider 供说明 / 呈现，并在返回点再次 clamp：当 `authorizable=false` 时，无论 decider、`Yolo` 或异常 `AllowAlways` 返回什么，最终均为 `Deny(NetworkUnauthorizable(preview.denial_reason))`。此 Deny MUST 发生在 `tool.execute` 前并把 reason 送入 is_error history / 续跑。

#### Scenario: 未知 Network 工具无论 decider 结果均保留系统拒绝原因

- **WHEN** 未 override preview 的 Network Tool 产生 default `authorizable=false`,decider 分别返回 Allow 或 Deny
- **THEN** decider 收到同一 reject-only preview供说明，但两次 gate 最终均返回 `Deny(NetworkUnauthorizable(原 reason))`、工具不 execute；Deny 分支不得改写为 `UserDenied`

#### Scenario: 畸形 Network 参数在 Yolo 仍拒绝

- **WHEN** 当前 Yolo,Network Tool 因必要参数畸形返回 `authorizable=false`
- **THEN** 不命中 auto Allow；最终 Deny,零 DNS / HTTP / WebFetcher

#### Scenario: 异常 AllowAlways 不能绕过不可授权状态

- **WHEN** 不可授权 Network preview 的 decider 异常返回 AllowAlways 对应的 Allow
- **THEN** gate 仍 clamp 为 Deny,不记忆、不落盘、不执行

#### Scenario: 有效专用 preview 才进入模式矩阵

- **WHEN** Network Tool 返回 `authorizable=true` 与 canonical target / scope
- **THEN** Normal / AcceptEdits / Plan 进入逐次询问,Yolo 可自动 Allow,且 preview 不被前端重算
