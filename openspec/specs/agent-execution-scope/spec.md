# agent-execution-scope Specification

## Purpose
agent-execution-scope 定义一次 Agent run 可安全派生的运行边界：稳定 identity、parent→child 单向 cancellation、只能保持或收紧的 iteration/deadline/depth 预算，以及工具名与权限级 capability 子集。它同时规定 TUI/headless 产品入口显式创建 remaining child depth 为 1 的全能力 root，legacy library wrapper 则继续创建 depth 0 root，所有派生 child scope 的 depth 均为 0；本能力只定义安全边界与派生预算，不提供调度器、child session、token 总预算或新用户命令。

## Requirements
### Requirement: 每次 Agent run 具有稳定且可派生的执行身份

系统 SHALL 以 `RunIdentity` 标识一次 Agent run；identity MUST 含本次唯一 `run_id` 与可选 `parent_run_id`。root scope 的 `parent_run_id` MUST 为 `None`；由 parent scope 派生的 child scope MUST 取得不同于所有祖先的新 `run_id`，且 `parent_run_id` 精确指向直接 parent。scope clone 仅用于共享同一次 run 的控制能力，MUST 保持同一 identity；既有无 scope 的 Agent 入口每次调用 MUST 创建新的 root identity。

#### Scenario: root 与 child identity 建立直接父子关系
- **WHEN** 创建一个 root scope 并从它派生一个 child scope
- **THEN** 两者 `run_id` 不同，root 的 `parent_run_id=None`，child 的 `parent_run_id=Some(root.run_id)`

#### Scenario: legacy 入口每次创建独立 root
- **WHEN** 连续两次调用既有无 scope Agent 入口
- **THEN** 两次 observer 所见 identity 均无 parent 且 `run_id` 互不相同

### Requirement: cancellation 沿父到子单向传播

execution scope SHALL 提供幂等 cancellation。取消 parent MUST 使其自身与所有已派生 descendant 进入 canceled；取消某个 child MUST 只取消该 child 及其 descendant，不得取消 parent 或 sibling。派生自已取消 scope 的 child MUST 立即处于 canceled。cancellation wait MUST 无丢失唤醒：无论 cancel 发生在 wait 注册前、注册中或注册后，等待者都必须完成。

#### Scenario: parent cancellation 传播到全部 descendant
- **WHEN** root 已派生两个 child，其中一个又派生 grandchild，随后取消 root
- **THEN** root、两个 child 与 grandchild 的 cancellation wait 全部完成

#### Scenario: child cancellation 不反向或横向传播
- **WHEN** root 派生 child-a 与 child-b，随后只取消 child-a
- **THEN** child-a 进入 canceled，root 与 child-b 保持未取消

#### Scenario: 取消先于等待仍可观测
- **WHEN** 先取消 scope，再开始等待其 cancellation
- **THEN** 等待立即完成，不发生永久挂起

### Requirement: 派生预算只能保持或收紧

scope SHALL 携带 `max_iterations`、可选 deadline 与 `remaining_child_depth`。Agent scoped run 的有效 iteration 上限 MUST 为 Agent 配置上限与 scope 上限的较小值。child 请求的 iteration 上限不得大于 parent；parent 有 deadline 时 child deadline MUST 不晚于 parent，parent 无 deadline时 child可继承无 deadline或设置有限deadline；每派生一级 MUST 消耗一个 child-depth，depth 为 0 时派生失败。任何扩大预算的派生请求 MUST fail-closed，不得静默接受扩大的值。

#### Scenario: child 使用更小 iteration 与更早 deadline
- **WHEN** parent 允许 20 iterations、deadline 为 T2、depth 为 1，派生请求为 8 iterations、deadline T1 且 T1<T2
- **THEN** child 派生成功，有效预算为 8、T1、depth 0

#### Scenario: child 不能扩大 parent 预算
- **WHEN** child 请求更大的 iteration 上限、更晚 deadline、移除 parent deadline或在 parent depth=0 时继续派生
- **THEN** 对应派生返回错误，child run 不会创建

#### Scenario: Agent 配置继续形成硬上限
- **WHEN** Agent 配置 `max_iterations=10` 而 root scope 请求 20
- **THEN** scoped run 的有效 iteration 上限为 10

### Requirement: capability 派生单调收窄

scope SHALL 以允许的 tool names 与 `PermissionLevel` 集合表达 capability。root capability 可覆盖其 registry 的全部工具与权限级；child capability MUST 同时是 parent tool-name 集合和 permission-level 集合的子集。请求未知工具、parent 未允许的工具、parent 未允许的权限级或重复工具名 MUST 返回派生错误，不得通过自动忽略或取并集继续。scope capability 与受限 registry 是两层防线：任一层拒绝都必须阻止 schema 下发、permission decision 与 execute。

#### Scenario: 只读 child capability 派生成功
- **WHEN** parent 允许全部内置工具与四个权限级，child 只请求 `list_dir/read_file/glob/grep` 和 `ReadOnly`
- **THEN** child 派生成功且 capability 精确为所请求子集

#### Scenario: capability 扩张 fail-closed
- **WHEN** child 请求 parent 未允许的工具或 `PermissionLevel`
- **THEN** 派生返回错误，不创建部分 capability

#### Scenario: 重复与未知工具名不被静默吞掉
- **WHEN** child capability 请求含重复名称或 registry 中不存在的名称
- **THEN** 派生返回可区分的错误，不生成 scope

### Requirement: deadline 形成可区分的运行终止原因

scope deadline 到达 MUST 使 scoped run 以 deadline-exceeded 终止；显式 cancellation MUST 以 canceled 终止，两者不得伪装成 Provider timeout、`MaxIterations` 或用户权限拒绝。已到期 scope 在进入 context preparation、Provider、permission 或 tool work 前 MUST 立即终止。legacy root scope 默认无 deadline，保持既有行为。

#### Scenario: deadline 到达与手工取消可区分
- **WHEN** 两次 scoped run 分别因 deadline 到达和显式 cancel 停止
- **THEN** 前者返回 deadline-exceeded，后者返回 canceled，二者均不返回 `ProviderError::Timeout`

#### Scenario: legacy root 无新增超时
- **WHEN** 调用既有 `run` / `run_observed` 且调用方未提供 scope
- **THEN** 不因 execution-scope 引入新的 deadline，既有 Provider attempt timeout 语义保持不变

### Requirement: 产品root显式开放单层child预算

TUI与headless产品入口 SHALL 为每个用户Prompt创建全能力、remaining child depth为1的root execution scope，以允许`delegate_task`派生单层child。既有公开`Agent::run`、`run_observed`与`root_scope`兼容入口 MUST 继续创建depth 0 root，scope-aware schema MUST 隐藏需要child depth的工具，dispatch MUST 拒绝模型硬发；调用方未显式选择产品委派入口时不得看到或获得child派生能力。所有child scope的remaining child depth MUST 为0。

#### Scenario: TUI与headless可派生一层child
- **WHEN** TUI或headless产品root处理一个有效`delegate_task`
- **THEN** root identity无parent、depth为1，派生child identity指向该root且child depth为0

#### Scenario: legacy wrapper保持depth零
- **WHEN** library调用方继续使用变更前的`Agent::run`、`run_observed`或`root_scope`
- **THEN** root预算depth仍为0，Provider schema不含`delegate_task`，硬发delegate在observer/permission/execute前fail-closed，其他Provider请求、history与observer逐项不变

#### Scenario: child派生不影响sibling
- **WHEN** 同一root并发派生多个depth 0 child且其中一个自行失败或取消
- **THEN** root与其他sibling保持未取消，只有parent cancellation可传播到全部child
