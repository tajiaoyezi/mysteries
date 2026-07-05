# tui-shell Delta

## MODIFIED Requirements

### Requirement: ChannelDecider 权限 oneshot 往返

`ChannelDecider` SHALL impl 既有 async `PermissionDecider`:`decide` MUST 先查注入的 `PolicyEngine`(见 `permission-gate` 的「命令 allowlist 自动放行」)——调用的 permission key 命中 allowlist 即返回 `Allow`、**不发起 channel 往返**;未命中且 `auto_allows(mode, level)` 未命中时,MUST 创建 `oneshot`、向 UI 发 `AgentEvent::PermissionRequired{tool_name, args, allow_always_key, responder}`(`allow_always_key: Option<String>` 为该调用的 permission key,`Some` 表示可 always-allow),在 `responder` 的 `rx.await` 处挂起,收到 `PermissionReply` 后映射为 `PermissionDecision` 返回:`AllowOnce → Allow`;`AllowAlways →` 记忆 + 持久化(见 `permission-gate` 的「always-allow 记忆与持久化」)`→ Allow`;`Deny`、或 UI 端 sender / responder 断开,MUST 返回 `PermissionDecision::Deny`(fail-safe)。`decide` 返回类型仍为既有 `PermissionDecision {Allow, Deny}`。本机制 MUST 不改动 `agent-loop`(经既有 `PermissionDecider` 缝接入)。

#### Scenario: 权限请求挂起-恢复

- **WHEN** `ChannelDecider::decide` 被调用(allowlist 与 `auto_allows` 均未命中),UI 收到 `PermissionRequired` 后经 `responder` 回送 `AllowOnce`
- **THEN** `decide` 返回 `Allow`(挂起在 `rx.await`、收到后恢复)

#### Scenario: UI 断开 fail-safe 拒绝

- **WHEN** `decide` 发出请求后 UI 端 responder 被丢弃(`rx` 出错)
- **THEN** `decide` 返回 `Deny`,不 panic

#### Scenario: allowlist 命中不发起 channel 往返

- **WHEN** 调用的 permission key 已在注入 `PolicyEngine` 的 allowlist 中
- **THEN** `decide` 直接返回 `Allow`,不创建 `oneshot`、不发 `PermissionRequired`

## ADDED Requirements

### Requirement: 命令类权限框 always-allow 选项

C6 权限框 SHALL 在 `pending_permission` 的 `allow_always_key` 为 `Some` 时,于既有 `[y·允许][n·拒绝]` 间加 `[a·总是允许]` 选项;`allow_always_key` 为 `None`(如 `Edit` 类无 command key)时 MUST NOT 显示该选项(既有 keyless 权限框渲染不变)。按键:`y` → `PermissionReply::AllowOnce`、`n` / `Esc` → `Deny`、`a`(仅 key 存在)→ `AllowAlways`。

#### Scenario: 命令类权限框含 always-allow 且带色快照

- **WHEN** 以带 `command` 参数的 `Execute` 工具触发权限框(`allow_always_key = Some`)渲染
- **THEN** 权限框含 `[y·允许][a·总是允许][n·拒绝]`,与锁定带色快照一致

#### Scenario: 无 key 权限框不含 always-allow

- **WHEN** 以无 `command`(如 `Edit` 的 `path`)的工具触发权限框(`allow_always_key = None`)
- **THEN** 权限框仅含 `[y·允许][n·拒绝]`,既有快照零 churn

#### Scenario: 按 a 回送 AllowAlways

- **WHEN** `allow_always_key = Some` 的权限框活跃时按 `a`
- **THEN** 经 `responder` 回送 `PermissionReply::AllowAlways`

### Requirement: 权限框 diff 按框高截断保动作行可见

C6 权限框的 diff body SHALL 按可用框高截断:以 `area.height` 减去边框与固定行(标题 / tool / args / 动作 / 提示)得 diff 预算,全量 diff 超预算时只显前若干行 + 末行「⋯ 其余 N 行」,确保动作行(`[y·允许]` … `[n·拒绝]`)与提示行**始终完整渲染在框内、不被裁**。既有短 diff(未超预算)MUST NOT 触发截断、渲染不变。

#### Scenario: 长 diff 截断且动作行可见

- **WHEN** 以一个产生超过可用框高的长 diff 的 `write_file` 触发权限框渲染
- **THEN** diff body 截断为末行「⋯ 其余 N 行」,动作行 `[y·允许][n·拒绝]` 与提示行仍完整渲染在框内

#### Scenario: 短 diff 不截断

- **WHEN** 以一个 diff 未超可用框高的工具触发权限框渲染
- **THEN** diff 全量渲染,不出现「⋯ 其余 N 行」,与既有快照一致
