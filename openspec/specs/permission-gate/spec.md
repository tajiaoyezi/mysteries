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

### Requirement: 命令 allowlist 自动放行

系统 SHALL 提供 `PolicyEngine`,持一份命令白名单(`allowed`,来源为 config 的 `allowed_commands`,见 `config-layering`)。`PermissionDecider` 的具体实现 MUST 在查 `auto_allows`(权限模式)与发起 UI 询问**之前**先查 `PolicyEngine`:调用的 **permission key** 命中白名单 → 直接 `Allow`、不发起 UI 往返。permission key MUST 为纯函数派生(v1:**仅 `Execute` 级工具**且 `call.arguments["command"]` 为 string 时 → `normalize` = trim + 内部连续空白压成单空格;否则 `None`);`is_allowed = permission_key(...).is_some_and(|k| allowed.contains(&k))`。无 key 的工具(如 `Edit` 的 `path`、或任何非 `Execute` 级)MUST NOT 命中,仍走既有模式/询问。allowlist 命中**先于**模式判定(精确命令白名单比模式更具体)。key 派生与匹配为 headless 纯逻辑,强制 TDD。`ReadOnly` 仍由门直接放行、与 PolicyEngine 无关。

#### Scenario: 命中白名单直接放行不询问

- **WHEN** 一条 `command` 命中 allowlist 的 `Execute` 工具调用经 decider 决策
- **THEN** 返回 `Allow`,且不发起 oneshot / UI 往返

#### Scenario: 未命中仍按模式与询问

- **WHEN** `command` 不在 allowlist、当前模式 `Normal`
- **THEN** 不被 PolicyEngine 放行,走既有 UI 询问

#### Scenario: normalize 消除空白差异

- **WHEN** allowlist 含 `"git status"`,调用 `command` 为 `"git   status"`(多空白)
- **THEN** 规范化后相等,命中放行

#### Scenario: 无 command 字段不命中

- **WHEN** 一个 `Edit` 工具调用(args 为 `path`、无 `command`)
- **THEN** `permission_key` 为 `None`、`is_allowed` 为 `false`

### Requirement: always-allow 记忆与持久化

当决策发起 UI 询问且该调用存在 permission key 时,UI 询问 SHALL 提供 always-allow 选项;回复类型为 `PermissionReply {AllowOnce, AllowAlways, Deny}`。收到 `AllowAlways` 时系统 MUST:①允许本次(映射为 `Allow`);②把 key 记入内存 `PolicyEngine`(本会话即时生效);③持久化到 **user config 的 `allowed_commands`**(见 `config-layering`、dedup)。此后同 key 的调用 MUST 经「命令 allowlist 自动放行」直接放行。持久化失败 MUST 非致命:产出 `Notice`、内存记忆仍生效、判定仍 `Allow`、不 panic。key 不存在时 UI MUST NOT 提供该选项(`AllowAlways` 若仍到达则退化为 `AllowOnce`、不落盘)。`PermissionReply::Deny` 或 responder 断开 → `Deny`(fail-safe 不变)。`gate` 返回类型仍为既有 `PermissionDecision {Allow, Deny}`、MUST NOT 新增变体。

#### Scenario: always-allow 落盘后同命令自动放行

- **WHEN** 权限询问回送 `AllowAlways`(key = `"cargo build"`)
- **THEN** 本次返回 `Allow`,该 key 入内存 PolicyEngine 且写入 user config `allowed_commands`;之后同 `command` 的调用不再发起 UI 往返(经 allowlist 直放)

#### Scenario: 持久化失败非致命

- **WHEN** 回送 `AllowAlways` 但写 config 失败
- **THEN** 仍返回 `Allow`、产出一条 `Notice`、内存仍记住该 key(本会话不再问),不 panic

#### Scenario: 无 key 不提供 always-allow

- **WHEN** 询问的工具无 permission key(如 `Edit` 的 `path`)
- **THEN** 询问不提供 always-allow 选项;即便 `AllowAlways` 到达也退化为 `Allow`、不落盘

#### Scenario: 拒绝与断开仍 Deny

- **WHEN** 回送 `PermissionReply::Deny`,或 responder 被丢弃
- **THEN** `decide` 返回 `Deny`(fail-safe)

