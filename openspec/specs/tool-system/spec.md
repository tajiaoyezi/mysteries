# tool-system Specification

## Purpose
TBD - created by archiving change add-agent-loop-core. Update Purpose after archive.
## Requirements
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

### Requirement: 工具 schema 供下发

`ToolRegistry` SHALL 能产出已注册工具的 schema 列表,每项含 `name` / `description` / `parameters`,供 Loop 放入 `ModelRequest.tools` 下发给模型(§5.3 `schema()` 即「喂模型的 JSON Schema」)。

#### Scenario: 产出 schema 列表

- **WHEN** 注册两个 mock `Tool` 后向 registry 索取 schema 列表
- **THEN** 返回两项,各含对应工具的 `name` / `description` / `parameters`

### Requirement: 注册表拒绝重名工具

`ToolRegistry::register` SHALL 在工具名已存在时返回 `Err`(重名),不覆盖原有工具;名字未占用时返回 `Ok`。既有的按名注册 / 查找 / `schemas()` 行为不变;实现保留 `Vec` 以维持 `schemas()` 的插入顺序(供模型请求的工具顺序确定)。

#### Scenario: 重名注册被拒

- **WHEN** 用一个已注册过的名字再次 `register`
- **THEN** 返回 `Err`,registry 中保留原工具(不被覆盖)

#### Scenario: 唯一名注册成功

- **WHEN** 用一个未占用的名字 `register`
- **THEN** 返回 `Ok`,该工具可被 `get` 查到

### Requirement: 工具退出码

`ToolOutcome` SHALL 增 `exit: Option<i32>`:进程类工具(执行外部命令)设其为进程退出码,其余工具 MUST 为 `None`。既有 `content` / `is_error` / `truncated` 字段与其语义 MUST 不变(`exit` 默认 `None`,behavior-preserving)。

#### Scenario: 默认 None,进程类设码

- **WHEN** 构造一个非进程类工具的 `ToolOutcome`
- **THEN** 其 `exit` 为 `None`(既有字段行为不变)
- **WHEN** 进程类工具以退出码 0 结束
- **THEN** 其 `ToolOutcome.exit` 为 `Some(0)`

