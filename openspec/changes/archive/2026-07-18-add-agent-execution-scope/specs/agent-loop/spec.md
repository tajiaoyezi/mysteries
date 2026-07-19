## ADDED Requirements

### Requirement: Agent Loop 提供 scoped run 入口并保持 legacy 兼容

`Agent` SHALL 提供显式接收 execution scope 的 scoped run / observed run 入口。既有 `run` / `run_observed` MUST 作为兼容 wrapper，为每次调用创建无 deadline、无 child capability 扩张入口的 root scope，并委托同一 scoped 实现；除 observer 新增的 run identity 外，未取消 legacy 调用的 history、schema、Provider 请求、tool 调度、返回值与错误分流 MUST 保持不变。

#### Scenario: scoped 与 legacy 正常完成结果等价
- **WHEN** 用相同 Mock Provider 脚本分别调用 legacy 与等价 root scoped 入口且均不取消
- **THEN** 两次的 Provider 请求、history、最终文本及 tool outcome 逐字段一致

#### Scenario: legacy wrapper 不复用上一轮 scope
- **WHEN** 同一个 Agent 先后执行两个 legacy run
- **THEN** 两次各自使用新的 root identity，前一轮 cancellation state 不影响后一轮

### Requirement: cancellation 在 Agent Loop 内确定性收口

scoped run MUST 在 context preparation、每次 Provider 请求、permission decision、串行 tool execute、并行批次等待及 forced-final 请求处同时等待 scope termination。若 cancellation 或 deadline 在当前 `User` 之后、任何 `Assistant` 提交之前发生，Loop MUST 从模型 history 回滚该未提交的当前 `User` turn，使下一轮 Provider 请求不再携带旧任务；TUI transcript MAY 保留用户输入与 interrupted 展示。若 termination 在某条 `Assistant.tool_calls` 写入 history 后发生，Loop MUST 保留当前 `User`、该 `Assistant` 与已按 occurrence 发布的结果，并为该 Assistant 中每个尚未发布的 occurrence 按模型顺序追加且仅追加一个 is_error `ToolResult`；canceled 与 deadline-exceeded MUST 使用可区分的稳定内容。尚未启动的工具不得启动，已启动但未发布的 future 必须被 drop，其迟到结果不得进入 history 或 observer；已进入 blocking pool 的无副作用工作 MAY 自然结束，但结果仍必须丢弃。cancellation只约束Agent编排、future与结果发布，MUST NOT宣称回滚已发生的工具副作用或保证终止已由工具启动的OS进程。收口后 scoped run MUST 返回独立 `ScopedAgentError::{Cancelled,DeadlineExceeded}`（普通Agent错误以另一个variant包装），不得给既有公开 `AgentError` 增variant，不得请求下一轮 Provider或进入 forced-final。

#### Scenario: Provider 等待期间取消不产生半条 Assistant
- **WHEN** scoped run 正等待 Provider 首次回复时被取消
- **THEN** Provider future 被 drop，history 不新增该未完成回复的 `Assistant`并回滚当前未提交的`User`，run 返回 canceled，后续 Provider 请求不携带旧任务

#### Scenario: 串行工具期间取消补齐当前及后续 occurrence
- **WHEN** Provider 已返回 `[call-1,call-2]`，call-1 execute 等待期间 scope 被取消
- **THEN** call-1 与 call-2 按模型顺序各得到一个 canceled is_error `ToolResult`，call-2 不执行，不再请求 Provider

#### Scenario: 并行批次取消保留已发布前缀并取消其余
- **WHEN** 一个并行安全批次的前缀结果已发布，后续 occurrence 尚未发布时 scope 被取消
- **THEN** 已发布前缀原样保留，其余每个 occurrence 按原顺序得到 canceled is_error `ToolResult`；未发布的物理完成结果不得越过 cancellation

#### Scenario: deadline 在权限等待期间收口
- **WHEN** 非 ReadOnly 工具正等待 decider 且 scope deadline 到达
- **THEN** permission future 被 drop，当前与后续未发布 occurrence 得到 deadline-exceeded ToolResult，工具不执行，run 返回 deadline-exceeded

#### Scenario: forced-final 也可取消
- **WHEN** 主循环用尽 iteration 后正等待 forced-final Provider 请求且 scope 被取消
- **THEN** forced-final future 被 drop，run 返回 canceled 而不是 `MaxIterations`，history 不写入半成品 Assistant

### Requirement: observer 事件携带 run identity 且取消后静默

scoped observed run 发出的每个 status、tool started、tool finished 与 usage 事件 MUST 经新增的 scoped observer callback 携同一 `RunIdentity`；不同 child run 即使共享 observer，也必须能按 identity 区分。既有 `AgentObserver` 方法签名 MUST 保持不变；新增 scoped callback MUST 有默认实现并转发到对应legacy callback，使已有observer实现无需修改仍能收到原事件。cancellation/deadline 被 Loop 接受后 MUST 不再发新的 tool-finished、usage 或 `Idle` 事件；synthetic interrupted ToolResult 只用于 history 协议收口，不伪装为实际工具完成。正常 legacy run 的事件相对顺序与既有契约保持不变。

#### Scenario: 并发 run 的 observer 事件可归属
- **WHEN** 两个不同 scoped run 共享同一个 recording observer并交错产生事件
- **THEN** 每个事件均可按 `run_id` 归入唯一 run，child 事件还可由 `parent_run_id` 关联直接 parent

#### Scenario: cancellation 后无迟到 observer 事件
- **WHEN** tool started 后取消 scope
- **THEN** observer 不再收到该 run 的 tool finished、usage 或 Idle；迟到 blocking result 也不产生事件

#### Scenario: legacy observer 实现保持source-compatible
- **WHEN** 一个既有observer只实现变更前的`on_status/on_tool_call_started/on_tool_call_finished/on_usage`
- **THEN** 代码无需增加新方法即可编译，并经scoped callback默认适配收到与变更前相同的事件

### Requirement: TUI turn 使用内核 cancellation 收口

TUI `run_agent_task` SHALL 为每个 Prompt 创建新的 root execution scope。Interrupt 到达时 SHALL 取消该 scope并等待 scoped run 完成 Agent 内部 history 收口，再保存 working history与发送唯一 `Interrupted`；不得继续依赖“drop run future后由 TUI suffix helper补当前 turn”作为主路径。旧 session 激活时的历史 normalization MUST 保留，用于兼容升级前已持久化的 dangling occurrence。此接线不得改变现有 TUI 布局、session JSONL schema、Running 卡收口文案、排队推进或“Interrupted 后无 trailing finished / Idle”行为。

#### Scenario: TUI interrupt 保存 Agent 已收口 history
- **WHEN** TUI turn 中两个工具调用尚未完成时触发 Interrupt
- **THEN** 保存的 working history 由 scoped run 为每个未完成 occurrence 补齐 canceled ToolResult，只发送一次 `Interrupted`，无 trailing finished / Idle，随后排队 Prompt 可正常运行

#### Scenario: Provider 回复前中断不污染下一轮 Prompt
- **WHEN** TUI 在 Provider 返回首条 `Assistant` 前中断当前 Prompt，随后提交或推进下一条 Prompt
- **THEN** transcript 保留旧 Prompt 与唯一 Interrupted 展示，但下一轮 Provider 请求不含旧 Prompt，只回答新的待处理 Prompt

#### Scenario: 旧 session normalization 继续兼容
- **WHEN** 激活一个升级前保存且含 dangling tool call 的 session
- **THEN** activation normalization 仍补齐旧 occurrence；本 change 不修改磁盘 wire或 raw load round-trip
