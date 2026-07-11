## MODIFIED Requirements

### Requirement: 多轮编排循环

系统 SHALL 提供 Agent Loop:从初始 history(System + User)出发,每轮以**完整 history** 请求 provider,将回复的 text 与 tool_calls 落为一条 `Assistant` 消息入 history;若该回复无 tool_calls,循环 SHALL 终止并返回最终回复文本;若有 tool_calls,则按 `ToolConcurrency` 处理为连续 `ParallelSafe` 批次与 `Exclusive` 单项屏障，每个调用仍先过既有纵深检查 / permission gate，再将结果按模型原始 `tool_calls` 顺序作为 `ToolResult` 入 history。当前回复的全部工具结果完整入 history 后，Loop 才能带累积 history 发下一次 provider 请求。6 类事件(用户输入、模型文本、工具调用、工具结果、权限拒绝、错误)MUST 全部映射进 history 的 `Message`(§5.5)。

#### Scenario: 无 tool_calls 单轮终止

- **WHEN** provider 首个回复不含 tool_calls
- **THEN** 循环返回该回复文本,history 末尾为对应 `Assistant` 消息,且不再发起请求

#### Scenario: 含工具的多轮编排

- **WHEN** provider 第一轮返回一个 tool_call、第二轮返回无 tool_call 的文本
- **THEN** 依次发生:`Assistant{tool_calls}` 入 history → 工具结果 `ToolResult` 入 history → 带累积 history 再请求 → `Assistant{text}` 入 history并终止;且第二次请求携带的 history 包含第一轮的全部消息

#### Scenario: 多工具回复先完整收口再请求下一轮

- **WHEN** provider 第一轮返回多个 tool_calls，其中含一个并行安全批次
- **THEN** 第二次 provider 请求只在每个 tool-call occurrence 均已有且仅有一个 `ToolResult` 后发起；即使 call id 重复也按 occurrence 数量配对，请求中的 ToolResult 顺序与第一轮 tool_calls 顺序一致

### Requirement: 结构化观测事件(observer 变体)

系统 SHALL 提供 `AgentObserver`(`Send + Sync`,方法 `on_status` / `on_tool_call_started` / `on_tool_call_finished` / `on_usage`,**全部 default no-op**)与 `AgentStatus`(`Idle` / `CallingModel` / `ExecutingTool(String)` / `ExecutingTools(usize)` / `WaitingForPermission`),以及 `Agent::run_observed(history, ctx, sink, observer)`:在循环关键点经 observer 发结构化事件 —— 模型调用前 `StatusChanged(CallingModel)`；独占 / 单项工具分发时发 `on_tool_call_started{id, name, args, readonly}` 与 `StatusChanged(ExecutingTool(name))`；长度大于 1 的并行批次按模型顺序发完该段所有 started 后发 `StatusChanged(ExecutingTools(count))`，其中 count 是整段已调度 occurrence 总数、不是瞬时 active 数，窗口补位不重复发 status。其中 `readonly` MUST 精确等价于 `permission_level == ReadOnly`(`Network` 为 false,不得标为“自动运行”)；`Network` / `Edit` / `Execute` 工具询问前发 `WaitingForPermission`(命中 mode 自动放行时可无等待事件)；工具 outcome 按原始 occurrence 顺序经 `on_tool_call_finished{id, outcome}` 上报(执行结果 / UserDenied / NetworkUnauthorizable / 未知工具均以 `ToolOutcome` 上报)；循环自然终止前发 `Idle`。每次 `provider.complete` 返回后,若 `ModelResponse.usage` 为 `Some`,MUST 经 `observer.on_usage(&usage)` 上送该轮真实 token 用量；`usage` 为 `None` 的轮 MUST NOT 上送。`on_usage` 取 `&Usage`(`provider-abstraction` 已定义),**default no-op**。

既有 `Agent::run` 的契约(history 累积、终止条件、错误分流,见本能力既有 requirement)MUST 保持不变,且 `run` MUST 委托 `run_observed` 并传入 no-op observer。单个工具调用继续使用 `ExecutingTool(name)`，不得退化为 count=1 的批次状态；`AgentObserver` 方法的 default no-op MUST 使任何不关心观测的调用方零负担。

#### Scenario: 观测一轮工具调用的事件顺序

- **WHEN** 以 Mock 脚本「轮1 → 一个工具的 tool_call、轮2 → 终复文本」调用 `run_observed`,传入一个记录事件的 observer
- **THEN** observer 依次收到 `CallingModel` → `ToolCallStarted` →(`ExecutingTool` / 必要时 `WaitingForPermission`)→ `ToolCallFinished` → 下一轮 `CallingModel` → `Idle`,且 history 与 `run` 等价(`on_usage` 是否触发取决于脚本是否带 usage,不改变上述 status / tool 事件的相对顺序)

#### Scenario: 观测并行批次的确定顺序

- **WHEN** 模型按 `[call-1, call-2]` 返回两个 eligible `ParallelSafe` 调用，第二项物理执行先完成
- **THEN** observer 仍依次收到两个按模型顺序的 `ToolCallStarted` → `ExecutingTools(2)` → `ToolCallFinished(call-1)` → `ToolCallFinished(call-2)`；不得把物理完成顺序暴露为 history / observer 顺序

#### Scenario: run 委托后行为与原一致(零回归)

- **WHEN** 调用既有 `Agent::run`(不带 observer)跑一个只含 `Exclusive` 或单个工具的既有脚本
- **THEN** 其 history、返回值、终止 / 错误行为与本 change 前完全一致(`run` 委托 `run_observed` + no-op observer,既有 agent-loop 测试保持绿)

#### Scenario: Network observer 不误报 ReadOnly

- **WHEN** 模型发出一个 `PermissionLevel::Network` 的 tool_call
- **THEN** `on_tool_call_started.readonly == false`;需要询问时 observer 收到 `WaitingForPermission`,不得产生“只读 · 自动运行”语义

#### Scenario: 权限拒绝仍上报工具完成

- **WHEN** 某非 `ReadOnly`(`Network` / `Edit` / `Execute`)工具被 decider 拒绝
- **THEN** observer 收到 `WaitingForPermission` 后,该工具以 is_error 的 `ToolOutcome`(user denied)触发 `on_tool_call_finished`,且既有「denial 入 history、循环继续」行为不变

#### Scenario: 每轮 model 调用后上送 token 用量

- **WHEN** 以 Mock 脚本(其 `ModelResponse` 带 `usage: Some(Usage{..})`)调用 `run_observed`,传入记录事件的 observer
- **THEN** 该次 model 调用返回后 observer 收到 `on_usage` 携该轮 `Usage`;若某轮 `ModelResponse.usage` 为 `None` 则该轮不收到 `on_usage`;`run`(no-op observer)行为不受观测机制影响

## ADDED Requirements

### Requirement: 顺序稳定的有界安全批次

Agent Loop SHALL 只把最大连续、全部满足 `tool exists && concurrency()==ParallelSafe && permission_level()==ReadOnly && plan_only()==false` 的调用段作为并行批次；任一未注册、`Exclusive`、非 `ReadOnly` 或 `plan_only` 调用 MUST 形成屏障并走既有串行路径。批次固定最多同时 poll `MAX_PARALLEL_TOOL_CALLS = 4` 个 execute future，不新增配置字段；前批完整结束前不得执行屏障，屏障结束前不得启动后批。段内调度 MUST work-conserving：任一物理完成项空出窗口后，即使更早 index 尚未完成，也要允许下一待执行项补位；公开结果仍由独立有序 ready buffer 控制。

批次内每个调用仍 MUST 经过既有 lookup、mode 纵深检查与 permission gate。首版 eligible 条件保证不会并发用户授权；即使未来某个 Network / Edit / Execute Tool 错误声明 `ParallelSafe`，host clamp 也 MUST 将其按 `Exclusive` 处理。

#### Scenario: 两个安全工具在释放前真实重叠

- **WHEN** 两个 `ParallelSafe` mock Tool 分别发送 per-call entered ack 后等待各自 release oneshot，模型在同一连续段调用二者
- **THEN** 测试可在发送任一 release 前收到两个 entered ack 且观察到 active==2；失败路径必须 release / abort driver，不得用 sleep 耗时推断重叠

#### Scenario: 五个安全调用最多同时执行四个

- **WHEN** 同一连续段含 5 个受控 `ParallelSafe` 调用并记录 max-active
- **THEN** max-active 恰为 4，第 5 个只在前四项至少一项释放后进入 execute

#### Scenario: 慢队首不阻塞第五项补位

- **WHEN** 同一段含 5 个调用，call-1 保持未 release，call-2 已完成并发出 completed ack
- **THEN** call-5 在 call-1 release 前发出 entered ack，max-active 仍≤4；call-2 outcome 只进 ready buffer，公开 history 仍等待 call-1

#### Scenario: Exclusive 是不可跨越的屏障

- **WHEN** 模型按 `[safe-1, safe-2, exclusive-3, safe-4]` 返回调用，各项用独立 entered / release / completed oneshot 控制
- **THEN** safe-1 / safe-2 可重叠；二者全部完成后 exclusive-3 才开始；exclusive-3 完成后 safe-4 才开始

#### Scenario: unknown tool 是不可跨越的屏障

- **WHEN** 模型按 `[safe-1, unknown-2, safe-3]` 返回调用
- **THEN** safe-1 完成并发布后才产生 unknown-2 的 is_error ToolResult，unknown-2 收口后 safe-3 才 started；safe-1 / safe-3 不得重叠

#### Scenario: plan_only 即使标 ParallelSafe 仍是屏障

- **WHEN** Plan 模式中的测试 Tool 同时报 `ParallelSafe + ReadOnly + plan_only`，位于两个普通 safe 调用之间
- **THEN** 该 plan_only 调用按 `Exclusive` 串行执行，前后 safe 调用不得跨越；非 Plan 下仍沿用既有纵深拒

#### Scenario: 单个 ParallelSafe 调用保持单项路径

- **WHEN** 一个连续安全段长度为 1
- **THEN** 只执行一次工具并使用 `ExecutingTool(name)`，history / observer 与 change 前单工具脚本一致

#### Scenario: 权限工具即使误标 ParallelSafe 仍被 clamp

- **WHEN** 一个测试 Tool 同时报 `concurrency=ParallelSafe` 与 `permission_level=Network`，并与另一调用相邻
- **THEN** 它形成 `Exclusive` 屏障、仍单独经过 Network gate；不得进入安全批次或产生并发 permission request

### Requirement: 并行结果顺序与错误隔离

并行批次 SHALL 允许物理完成顺序与模型顺序不同，并以 original index 将 outcome 暂存到 ready buffer；`ToolResult` 写入、`on_tool_call_finished` 上报及下一轮 Provider 可见顺序 MUST 与原 `tool_calls` occurrence 顺序完全一致，每个 occurrence 恰好一个结果。call id 不保证唯一，重复 id MUST 产生对应数量的 ToolResult / finished 回调。任一调用返回 `ToolOutcome.is_error=true` MUST 只把该项编码为 is_error `ToolResult`，不得取消尚未完成的兄弟调用；批次全部收口后循环继续。

#### Scenario: 逆序完成仍按模型顺序入 history

- **WHEN** 模型顺序为 `[call-1, call-2]`，用各自 release / completed oneshot 控制 call-2 先产生 outcome、call-1 后产生 outcome
- **THEN** history 与 observer finished 均为 call-1 → call-2；下一轮 Provider 实收 messages 中顺序相同

#### Scenario: 重复 call id 按 occurrence 产出结果

- **WHEN** 同一批次两个不同 args 的 tool-call occurrence 复用同一 `call-1` id
- **THEN** 两个调用都执行，history 按 occurrence 顺序含两个 `call_id=call-1` 的 ToolResult，observer 也收到两次 finished；不得把 id 当去重键

#### Scenario: 单项失败不取消同批其他项

- **WHEN** 三个安全调用同批执行，其中第二项返回 `is_error=true`、其余成功
- **THEN** 三项都执行且各产生一个 ToolResult；仅第二项 `is_error=true`，循环在整批结束后继续请求 Provider
