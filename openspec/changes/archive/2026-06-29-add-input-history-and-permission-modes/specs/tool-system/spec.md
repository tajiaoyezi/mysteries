## MODIFIED Requirements

### Requirement: 工具抽象与注册表

系统 SHALL 定义 `Tool` trait(`name` / `description` / `schema` / `permission_level` / `execute(args, ctx) -> ToolOutcome`,async,dyn 安全)、`ToolRegistry`(按名注册与查找)、`ToolOutcome{content, is_error, truncated}`、`ToolContext{cwd, max_output_bytes}`、`PermissionLevel{ReadOnly, Edit, Execute}`。`Edit` 表文件改动类工具(写 / 编辑),`Execute` 表命令执行类工具(shell);二者均需确认(在 `normal` 模式下),`accept-edits` 模式仅自动放行 `Edit`(详见 permission-gate)。本 change 不含实体工具实现(以 in-test mock `Tool` 驱动测试)。

#### Scenario: 注册与按名分发

- **WHEN** 向 registry 注册一个 mock `Tool` 并以其 `name` 查找
- **THEN** 取得该 tool,可对其 `execute` 得到 `ToolOutcome`

#### Scenario: 按名查找未注册工具

- **WHEN** 以一个未注册的名字查找 registry
- **THEN** 返回「不存在」(None),不 panic

#### Scenario: 工具声明改动类别

- **WHEN** 查询文件写 / 编辑类工具的 `permission_level`
- **THEN** 返回 `Edit`
- **WHEN** 查询命令执行类工具(shell)的 `permission_level`
- **THEN** 返回 `Execute`
