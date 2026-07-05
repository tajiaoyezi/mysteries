# tool-system Specification

## Purpose
tool-system 定义 Agent 可用工具的统一抽象与分发基础:`Tool` trait、按名注册与查找的 `ToolRegistry`、执行结果 `ToolOutcome` 与执行环境 `ToolContext`,并产出 schema 列表供 Loop 放入 `ModelRequest.tools` 下发给模型。设计立场是工具自声明 `permission_level`(`ReadOnly` / `Edit` / `Execute`)作为权限判定的输入,registry 拒绝重名并保持插入顺序,使下发给模型的工具集确定。本域只提供抽象、注册与执行入口:何时调用由 agent-loop 编排,是否放行由 permission-gate 依 `permission_level` 裁决。
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

### Requirement: Tool::plan_only 与 mode-aware schema 下发(schema-omit)

`Tool` SHALL 提供 `fn plan_only(&self) -> bool`(default `false`);标记「仅 Plan 模式有意义」的工具(如 `submit_plan`)override 为 `true`。`ToolRegistry` SHALL 提供 `schemas_for(mode: PermissionMode) -> Vec<schema>`:
- **`Plan` 模式**:仅含 `permission_level()==ReadOnly || plan_only()` 的工具(只读研究工具 + plan_only 工具),摘掉 `Edit`/`Execute` 类(schema-omit)。
- **非 `Plan` 模式**:仅含 `!plan_only()` 的工具(全部除 plan_only 类——plan_only 工具在别模式无意义、不下发)。

两路 MUST 维持既有插入顺序。既有 `schemas()`(不分 mode)行为 MUST 不变(behavior-preserving)。`plan_only` 默认与 `schemas_for` 过滤为 headless 纯逻辑,强制 TDD。

#### Scenario: Plan 模式摘变更类、留只读 + plan_only

- **WHEN** registry 依次含 ReadOnly / Edit / Execute / plan_only 各一,取 `schemas_for(Plan)`
- **THEN** 仅含 ReadOnly 与 plan_only 两项(Edit/Execute 被摘),顺序保持

#### Scenario: 非 Plan 模式摘 plan_only

- **WHEN** 取 `schemas_for(Normal)`(或 AcceptEdits / Yolo)
- **THEN** 含 ReadOnly / Edit / Execute,不含 plan_only 项

#### Scenario: plan_only 默认 false

- **WHEN** 查一个未 override 的普通工具的 `plan_only()`
- **THEN** 为 `false`

