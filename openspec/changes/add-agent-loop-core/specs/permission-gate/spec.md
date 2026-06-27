## ADDED Requirements

### Requirement: 可注入的权限门

系统 SHALL 提供权限门:`ReadOnly` 工具直接放行(不询问);`RequiresConfirmation` 工具经**可注入的 `PermissionDecider` seam**(async)取得 `Allow` / `Deny`。本 change MUST NOT 将门绑定到 §3 的 oneshot / UI channel;测试以注入式 decider 提供决策。门是集中决策点(后续 §13 1.3 PolicyEngine 接在此前)。

#### Scenario: 只读工具直接放行

- **WHEN** 对一个 `ReadOnly` 工具调用权限门
- **THEN** 返回 `Allow`,且不调用 `PermissionDecider`

#### Scenario: 变更工具经 decider 决策

- **WHEN** 对一个 `RequiresConfirmation` 工具调用权限门,并注入一个总是返回 `Allow` 的 decider
- **THEN** `PermissionDecider` 被调用,门返回 `Allow`

### Requirement: 拒绝产出 is_error ToolResult

被拒绝的工具调用 SHALL NOT 执行,且 MUST 产出一条 is_error 的 `ToolResult`(「user denied」类)入 history,循环据此继续(不静默跳过)。

#### Scenario: 拒绝 → denial 入 history 且续跑

- **WHEN** 注入一个总是返回 `Deny` 的 decider,Loop 处理一个 `RequiresConfirmation` 的 tool_call
- **THEN** 该工具不被执行,一条 `ToolResult{is_error: true}` 入 history,Loop 继续发起下一轮请求
