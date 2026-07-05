# permission-gate Delta

## ADDED Requirements

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
