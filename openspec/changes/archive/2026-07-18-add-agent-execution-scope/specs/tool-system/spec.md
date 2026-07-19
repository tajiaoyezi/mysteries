## ADDED Requirements

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
