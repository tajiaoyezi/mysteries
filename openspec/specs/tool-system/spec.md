# tool-system Specification

## Purpose
tool-system 定义 Agent 可用工具的统一抽象与分发基础:`Tool` trait、按名注册与查找的 `ToolRegistry`、执行结果 `ToolOutcome` 与执行环境 `ToolContext`,并按运行时 mode 与 execution capability 产出 schema 列表供 Loop 放入 `ModelRequest.tools`。工具分别自声明四类 `PermissionLevel` 与正交的 `ToolConcurrency::{Exclusive, ParallelSafe}`；并发分类默认 `Exclusive`，不得从 `ReadOnly` 推断，registry/host 只能收紧而不能提升。`ToolRegistry` 可用共享同一 `Arc<dyn Tool>` 实例的受限视图保留 parent 子集，schema 过滤和 dispatch clamp 共同防止 scope 扩权。Network 工具还须提供 tool-owned、零网络的结构化 preview,不可授权 preview 在所有 mode 下 fail-closed。registry 拒绝重名并保持插入顺序,Plan 只下发 ReadOnly + Network + plan_only,非 Plan 摘掉 plan_only。本域只提供抽象、注册与执行入口:何时及是否并行调用由 agent-loop 编排,是否放行由 permission-gate 裁决。
## Requirements
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
- **`Plan` 模式**:仅含 `permission_level()==ReadOnly || permission_level()==Network || plan_only()` 的工具(本地只读研究工具 + 需授权的网络研究工具 + plan_only 工具),摘掉 `Edit` / `Execute` 类(schema-omit)。
- **非 `Plan` 模式**:仅含 `!plan_only()` 的工具(全部除 plan_only 类——plan_only 工具在别模式无意义、不下发),其中包含 `Network`。

两路 MUST 维持既有插入顺序。既有 `schemas()`(不分 mode)行为 MUST 不变(behavior-preserving)。`plan_only` 默认与 `schemas_for` 过滤为 headless 纯逻辑,强制 TDD。

#### Scenario: Plan 模式摘变更类、留 ReadOnly + Network + plan_only

- **WHEN** registry 依次含 ReadOnly / Network / Edit / Execute / plan_only 各一,取 `schemas_for(Plan)`
- **THEN** 仅含 ReadOnly / Network / plan_only 三项(Edit/Execute 被摘),顺序保持

#### Scenario: 非 Plan 模式摘 plan_only、保留 Network

- **WHEN** 取 `schemas_for(Normal)`(或 AcceptEdits / Yolo)
- **THEN** 含 ReadOnly / Network / Edit / Execute,不含 plan_only 项,顺序保持

#### Scenario: plan_only 默认 false

- **WHEN** 查一个未 override 的普通工具的 `plan_only()`
- **THEN** 为 `false`

### Requirement: Network preview 由工具拥有且默认不可授权

系统 SHALL 定义结构化 `NetworkPermissionPreview{authorizable, full_args, canonical_initial_target, scope, denial_reason}`；其中 scope 至少表达 `max_redirects`、`may_cross_origin` 与 `ssrf_each_hop`。`authorizable=true` 时 canonical target / scope MUST 存在且 denial reason 为空；`authorizable=false` 时 denial reason MUST 非空，target / scope 不得被用于 Allow。`Tool::network_permission_preview(args)` MUST 为纯函数、确定性且零 DNS / HTTP / `WebFetcher`；default MUST 返回 `authorizable=false` 与 generic full args / 不可授权原因。generic fallback 仅用于解释拒绝,不得获得 Allow。

只有 Network 工具的专用实现成功解析必要参数、能确定 execute 将使用的 canonical initial target，且 scope 与 transport 的真实常量同源时，才可返回 `authorizable=true`。permission-gate、TUI 与 CLI MUST 消费该结构，不得按 tool name 重建 URL、DDG endpoint 或 redirect scope。非 Network 工具不因 default preview 改变既有行为。

#### Scenario: 未 override 的 Network 工具默认不可授权

- **WHEN** 一个声明 `PermissionLevel::Network` 的 mock Tool 未 override `network_permission_preview`
- **THEN** preview 为 `authorizable=false`,保留 terminal-safe 格式化所需的 full args 与原因,不得成为可授权 generic preview

#### Scenario: 畸形必要参数产生不可授权 preview

- **WHEN** 专用 Network 工具收到缺失、类型错误或无法确定 canonical target 的必要参数
- **THEN** preview 为 `authorizable=false`、零 DNS / HTTP / WebFetcher,并携不可授权原因

#### Scenario: 专用 preview 可授权且不执行网络

- **WHEN** 专用 Network 工具收到合法参数并调用 `network_permission_preview`
- **THEN** 返回 `authorizable=true`、canonical initial target 与真实 scope,且此过程不调用 execute / DNS / HTTP / WebFetcher

#### Scenario: 非 Network 工具不受 preview 默认值影响

- **WHEN** ReadOnly / Edit / Execute 工具沿用 default preview
- **THEN** 其既有 gate、mode 与执行行为不变；default 不可授权语义只约束 PermissionLevel::Network

### Requirement: ToolRegistry 可安全共享工具实例

`ToolRegistry` SHALL 允许多个 registry/view 共享同一 `Tool` 实例而不复制其内部状态；共享后 `get`、`schemas`、`schemas_for`、`ToolConcurrency`、permission level、plan-only 与 execute 行为 MUST 与原 registry 一致。既有 `register(Box<dyn Tool>)` 调用形状 MUST 保持可用，重名拒绝与插入顺序契约不变。

#### Scenario: 派生 registry 共享同一工具实例
- **WHEN** 注册一个带可观测内部计数的 Tool，再从 parent registry 派生含该工具的受限 registry并分别执行
- **THEN** 两个 registry 观察到同一累计状态，不产生两个独立 Tool 副本

#### Scenario: 既有 Box 注册入口兼容
- **WHEN** 既有调用方继续用 `register(Box::new(tool))`
- **THEN** 注册、按名查找、重名错误与 schema 插入顺序均与变更前一致

### Requirement: 受限 registry 精确保留 parent 子集

`ToolRegistry` SHALL 提供按 tool-name 请求受限 registry/view 的接口。请求成功时结果 MUST 只含所请求工具，并按 parent 原始插入顺序输出 schema，不按请求顺序重排；工具对象必须与 parent 共享。请求中任一名称未知、重复或不属于 parent 时 MUST 整体返回错误，不得产生部分 registry。空请求 MAY 成功并产生空 registry。

#### Scenario: 子集按 parent 顺序而非请求顺序
- **WHEN** parent 顺序为 `[list_dir,read_file,glob,grep]`，请求顺序为 `[grep,list_dir]`
- **THEN** 受限 registry 只含 `list_dir/grep`，schema 顺序为 `[list_dir,grep]`

#### Scenario: 未知或重复名称整体失败
- **WHEN** 请求包含未知名称或同一名称两次
- **THEN** 接口返回可区分错误，不返回已解析的部分 registry

#### Scenario: 空 registry 不暴露工具
- **WHEN** 以空名称集合派生受限 registry
- **THEN** `schemas` 与 `schemas_for` 为空，任何名称查询均返回 None

### Requirement: capability 过滤同时约束 schema 与分发

registry 为 execution scope 生成 schema 时 MUST 同时应用 mode-aware 过滤与 scope capability；两者取交集，顺序保持。只在 schema 中隐藏不构成安全边界，Agent dispatch 对模型硬发的已注册但 scope 禁止工具仍 MUST 进入 scope denial，不能调用其 permission decider或 execute。

#### Scenario: mode 与 scope 取交集
- **WHEN** Normal mode registry 含 ReadOnly/Network/Edit/Execute，而 scope 只允许两个 ReadOnly tool names与 `ReadOnly`
- **THEN** Provider schema 只含这两个工具且维持 parent 顺序

#### Scenario: 模型硬发被隐藏工具仍不能执行
- **WHEN** Provider 绕过 schema 硬发一个 registry 已注册但 scope 禁止的工具
- **THEN** Agent 产生 scope-denied ToolResult，decider与 tool execute 均不调用
