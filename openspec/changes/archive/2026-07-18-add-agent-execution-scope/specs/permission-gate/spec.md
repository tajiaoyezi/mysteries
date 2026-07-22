## ADDED Requirements

### Requirement: execution scope capability 先于所有允许路径

权限门 SHALL 提供接收 execution capability 的 scoped gate。scoped gate MUST 在 `ReadOnly` 直放、Network authorizability、`PolicyEngine`、`PermissionMode` 自动允许与 `PermissionDecider` 之前检查 tool name及 `PermissionLevel` 是否同时被 scope 允许；任一不允许 MUST 返回独立`ScopeViolation(reason)`错误，不调用 decider或 tool execute。既有无 scope `gate`、`PermissionGateOutcome`与`PermissionDenial` MUST 保持类型和variant不变；scoped gate以`Result<PermissionGateOutcome, ScopeViolation>`（或等价独立类型）包装现有结果，避免v1.3 minor破坏已有exhaustive match。

#### Scenario: ReadOnly 也受 scope clamp
- **WHEN** 一个 ReadOnly 工具存在于 registry但不在 scope allowed tools
- **THEN** scoped gate 返回 ScopeViolation，不因 ReadOnly 默认直放而执行

#### Scenario: Yolo 不能绕过 scope
- **WHEN** mode 为 Yolo 且 Edit/Execute/Network 工具被 scope 禁止
- **THEN** scoped gate 在 mode 自动允许前返回 ScopeViolation，decider/UI/execute 均不触发

#### Scenario: allowlist 不能绕过 scope
- **WHEN** Execute command 命中 `allowed_commands`，但 scope 未允许该工具或 Execute level
- **THEN** scoped gate 返回 ScopeViolation，allowlist 不产生 Allow

#### Scenario: root compatibility gate 保持原矩阵
- **WHEN** 既有调用方使用无 scope gate处理 ReadOnly、authorizable Network、Edit 与 Execute
- **THEN** 结果仍按既有 ReadOnly直放、Network clamp及 decider决定，不新增`PermissionDenial` variant

### Requirement: scope violation 以独立错误结果进入 history

系统 SHALL 新增独立`ScopeViolation(reason)`，MUST NOT 给既有`PermissionDenial`增加variant。Agent Loop 遇到该错误 MUST 在对应 occurrence 写入 is_error ToolResult并继续或按 scope termination 状态收口，但不得把它报告为 `UserDenied`、`NetworkUnauthorizable`、Plan拒绝或 unknown tool。reason MUST 可安全显示且至少指出 tool name或permission level越界，不得包含凭据。

#### Scenario: scope denial 与用户拒绝可区分
- **WHEN** 相同工具分别因 execution scope 禁止和用户在权限框选择 Deny而失败
- **THEN** 前者为 ScopeViolation content，后者保持 user denied content，两者均不执行工具

#### Scenario: scope denial 不触发 Network preview副作用
- **WHEN** scope 已禁止一个 Network 工具
- **THEN** 系统在 preview、decider和execute之前拒绝；不发生 DNS、HTTP 或 WebFetcher 调用
