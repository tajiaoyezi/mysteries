## MODIFIED Requirements

### Requirement: 工具抽象与注册表

系统 SHALL 定义 `Tool` trait(`name` / `description` / `schema` / `permission_level` / `concurrency` / `network_permission_preview(args)` / `execute(args, ctx) -> ToolOutcome`,async,dyn 安全)、`ToolRegistry`(按名注册与查找)、`ToolOutcome{content, is_error, truncated}`、`ToolContext{cwd, max_output_bytes}`、`PermissionLevel{ReadOnly, Network, Edit, Execute}` 与 `ToolConcurrency{Exclusive, ParallelSafe}`。

`Tool::concurrency()` SHALL 是 object-safe 纯函数并默认返回 `Exclusive`；`ParallelSafe` 是工具对“其 execute 可与其他同类调用重叠、不会因重叠破坏共享状态或产生顺序相关副作用”的显式正向声明。并发分类与权限分类 MUST 正交：`PermissionLevel` 只回答是否需要 Tool permission gate 授权，不得据此自动推断 `ToolConcurrency`；registry / host 只能把 `ParallelSafe` 收紧为 `Exclusive`，MUST NOT 把工具从 `Exclusive` 提升为 `ParallelSafe`。

`ReadOnly` 表该工具不产生需 Tool permission gate 授权的外部网络、文件写入或进程执行,因而可由权限门直接放行；交互 / 计划工具仍可通过各自 seam 发起用户交互或更新计划状态。`Network` 表工具执行会产生外部网络活动(不含 Agent 与 Provider 之间的模型协议 transport)；`Edit` 表文件改动类工具(写 / 编辑)；`Execute` 表命令执行类工具(shell)。Network 仅在 preview authorizable 后进入 mode matrix；有效 Network、Edit、Execute 在 `Normal` 均需确认，`AcceptEdits` 仅自动放行 Edit，`Yolo` 自动放行有效 Network / Edit / Execute；不可授权 Network 在所有 mode 下拒绝(详见 permission-gate)。

#### Scenario: 注册与按名分发

- **WHEN** 向 registry 注册一个 mock `Tool` 并以其 `name` 查找
- **THEN** 取得该 tool,可对其 `execute` 得到 `ToolOutcome`

#### Scenario: 按名查找未注册工具

- **WHEN** 以一个未注册的名字查找 registry
- **THEN** 返回「不存在」(None),不 panic

#### Scenario: 工具声明四类权限级别

- **WHEN** 查询本地文件读取 / 搜索类工具的 `permission_level`
- **THEN** 返回 `ReadOnly`
- **WHEN** 查询 `web_fetch` / `web_search` 的 `permission_level`
- **THEN** 返回 `Network`
- **WHEN** 查询文件写 / 编辑类工具的 `permission_level`
- **THEN** 返回 `Edit`
- **WHEN** 查询命令执行类工具(shell)的 `permission_level`
- **THEN** 返回 `Execute`

#### Scenario: Provider transport 不属于工具 Network 权限

- **WHEN** Agent 为一次模型推理调用已配置的 Provider HTTP transport
- **THEN** 该 transport 不经过 Tool permission gate；只有 `Tool::execute` 诱发的外部网络活动声明为 `Network`

#### Scenario: 交互与计划工具保持 ReadOnly

- **WHEN** 查询 `submit_plan` / `update_plan` / `ask_user` 的 `permission_level`
- **THEN** 三者仍返回 `ReadOnly`；其审批、内存计划更新或用户交互由各自 seam 管理,不因并发分类改 level

#### Scenario: 未 override 的 Tool 默认 Exclusive

- **WHEN** 一个 mock Tool 只实现既有必需方法、未 override `concurrency()`
- **THEN** 查询其并发分类得到 `ToolConcurrency::Exclusive`，注册与执行行为保持不变

#### Scenario: 并发分类不从 ReadOnly 推断

- **WHEN** 分别查询未 opt-in 的 `ReadOnly` 交互 Tool 与显式 opt-in 的 `ReadOnly` 本地读取 Tool
- **THEN** 前者为 `Exclusive`、后者为 `ParallelSafe`；两者的 `permission_level` 都仍为 `ReadOnly`

#### Scenario: registry 暴露 Tool 自身并发分类

- **WHEN** 注册一个 override `concurrency()==ParallelSafe` 的 mock Tool，再经 `ToolRegistry::get` 查回
- **THEN** 查回对象仍报告 `ParallelSafe`；registry 不按名称或权限级别重写为其他分类
