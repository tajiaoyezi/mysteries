# tool-system Specification

## Purpose
TBD - created by archiving change add-agent-loop-core. Update Purpose after archive.
## Requirements
### Requirement: 工具抽象与注册表

系统 SHALL 定义 `Tool` trait(`name` / `description` / `schema` / `permission_level` / `execute(args, ctx) -> ToolOutcome`,async,dyn 安全)、`ToolRegistry`(按名注册与查找)、`ToolOutcome{content, is_error, truncated}`、`ToolContext{cwd, max_output_bytes}`、`PermissionLevel{ReadOnly, RequiresConfirmation}`。本 change 不含实体工具实现(以 in-test mock `Tool` 驱动测试)。

#### Scenario: 注册与按名分发

- **WHEN** 向 registry 注册一个 mock `Tool` 并以其 `name` 查找
- **THEN** 取得该 tool,可对其 `execute` 得到 `ToolOutcome`

#### Scenario: 按名查找未注册工具

- **WHEN** 以一个未注册的名字查找 registry
- **THEN** 返回「不存在」(None),不 panic

### Requirement: 工具 schema 供下发

`ToolRegistry` SHALL 能产出已注册工具的 schema 列表,每项含 `name` / `description` / `parameters`,供 Loop 放入 `ModelRequest.tools` 下发给模型(§5.3 `schema()` 即「喂模型的 JSON Schema」)。

#### Scenario: 产出 schema 列表

- **WHEN** 注册两个 mock `Tool` 后向 registry 索取 schema 列表
- **THEN** 返回两项,各含对应工具的 `name` / `description` / `parameters`

