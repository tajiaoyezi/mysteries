# agent-loop Specification

## Purpose
agent-loop 是内核的多轮编排中枢:每轮以完整 history 请求 Provider,在轮顶固定一次 `PermissionMode` 快照并据此装配 mode-aware schemas、注入 transient Plan 指令及执行纵深拒，再经权限门处理回复中的 tool_calls。连续 `ParallelSafe + ReadOnly + !plan_only` 调用组成固定上限 4 的 work-conserving 安全批次，`Exclusive`、未知工具、非 `ReadOnly` 与 `plan_only` 均形成不可跨越屏障；物理完成可乱序，但 `ToolResult`、observer 与 Provider 可见顺序保持模型 occurrence 顺序。每个 tool occurrence 均通过只借用既有运行资源的 `ToolExecutionContext` 接收当前 execution scope、observer 与可选 read root，确保共享 registry 上的并发 run 不串上下文。`delegate_task` 作为普通 outer occurrence 参与有序收口：child-only failure 可恢复，parent termination 则在紧邻公开发布的 post-ready checkpoint 优先裁决并丢弃未发布 child outcome。每次 run 可由 execution scope 提供 identity、cancellation、iteration/deadline预算与 capability 上限；termination 在 Loop 内按已提交 Assistant/已发布 occurrence 边界确定性收口，TUI 只负责发出取消并消费唯一终态。设计立场是 history 为唯一事实源、工具失败或权限拒绝以 is_error `ToolResult` 入 history 续跑、provider 错误致命上抛；`max_iterations` 触顶时禁用工具追加一次强制收尾，并在该请求前发送`CallingModel`、响应后上送usage，且只在成功返回非空final时发送`Idle`。`AgentObserver` 的 `readonly` 仅等价于 `PermissionLevel::ReadOnly`，Network 不得误报自动运行；运行时 provider / model 切换及 no-op observer 均不改 `run` 既有契约。本域只负责编排次序、终止与错误分流,每轮实际发送的 messages 由 context-strategy 产出。
## Requirements
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

### Requirement: max_iterations 守卫

循环 MUST 受 `max_iterations` 限制(高位**安全网**,默认 50,仍可经配置覆盖),不得无限循环。循环跑满 `max_iterations` 轮仍未自然终止时,SHALL **不**直接以 `AgentError::MaxIterations` 终止,而是**追加一次** `provider.complete`、该次 `ModelRequest.tools` 传**空**(禁用工具),强制模型基于现有 history 产出文字回答:该次有文字则其 `Assistant{text}` 入 history 并返回 `Ok(text)`;仅当该次仍无文字(空 text 且无可用 tool_calls)时,才以致命错误 `AgentError::MaxIterations` 终止。强制收尾那次 `provider.complete` 自身返回 `Err` 时,按既有「provider 错误致命」分流为 `AgentError::Provider`。

#### Scenario: 触顶强制收尾产出文字

- **WHEN** provider 前 N 轮都返回 tool_call(永不自然终止)且 `max_iterations = N`,第 N+1 次调用(tools 已禁用)返回不含 tool_call 的文本
- **THEN** 第 N+1 次请求的 `ModelRequest.tools` 为空,其文本作为 `Assistant{text}` 入 history,循环返回 `Ok(text)`,不再发起请求

#### Scenario: 强制收尾仍无文字才致命兜底

- **WHEN** 跑满 `max_iterations` 轮后,强制收尾那次(tools 禁用)仍未产出文字
- **THEN** 循环以 `AgentError::MaxIterations` 终止

### Requirement: 可恢复错误与致命错误分流

工具执行失败(`ToolOutcome.is_error`)与未知工具名 SHALL 作为 is_error 的 `ToolResult` 入 history 且循环继续(可恢复);provider 返回的错误(本 change 无重试)SHALL 致命终止并以 `AgentError::Provider` 上抛。

#### Scenario: 工具失败可恢复

- **WHEN** 某 `tool.execute` 返回 `is_error = true` 的 `ToolOutcome`
- **THEN** 对应 `ToolResult{is_error: true}` 入 history,循环继续发起下一轮请求

#### Scenario: 未知工具名可恢复

- **WHEN** provider 返回的 tool_call 引用了未在 registry 注册的工具名
- **THEN** 产出一条 is_error 的 `ToolResult`(工具不存在)入 history,循环继续

#### Scenario: provider 错误致命

- **WHEN** `provider.complete` 返回 `Err`
- **THEN** 循环以 `AgentError::Provider` 终止,向上返回

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

### Requirement: 运行时模型切换

`Agent` SHALL 提供 `set_model(&mut self, model: String)`,更新后续 `ModelRequest.model` 所用模型。既有 `run` / `run_observed` 的 history / 终止 / 错误 / 事件行为 MUST 不变;`set_model` 只改「下次请求用哪个 model」,不影响进行中的轮。

#### Scenario: set_model 改后续请求的 model

- **WHEN** 对一个 `model = "m1"` 的 `Agent` 调 `set_model("m2")`,再跑一轮(Mock provider)
- **THEN** 该轮 `ModelRequest.model` 为 `"m2"`;其余循环行为与切换前一致(既有 agent-loop 测试保持绿)

### Requirement: system prompt 身份约束

`DEFAULT_SYSTEM_PROMPT` SHALL 含身份约束:禁止冒充 Claude / ChatGPT / OpenAI / Anthropic 或任何具体上游模型;被问及模型身份时,只说明运行于 Mysteries、所配置的模型名见状态行。该约束 MUST 由单测锁定关键短语(存在即绿,缺失即红)。

#### Scenario: 默认 system prompt 含身份约束短语

- **WHEN** 取 `DEFAULT_SYSTEM_PROMPT`
- **THEN** 其文本含 `Do not claim to be Claude`、`ChatGPT`、`OpenAI`、`Anthropic` 与「模型名见状态行」对应短语(`configured model name is shown in the status line`),任一缺失使单测失败

### Requirement: 运行时 provider 切换

`Agent` SHALL 支持运行时替换其 provider(`set_provider(Arc<dyn Provider>)`),对称于既有「运行时模型切换」。替换后,下一轮编排的模型请求 MUST 经新 provider 发出。为保持切换连贯,`set_provider` 与 `set_model` MUST 将新 provider / model 同步到 Agent 当前的 context strategy(携带 provider 的策略,如 `Compacting`,据此自动压缩走新 provider / model;不携带 provider 的策略,如 `Passthrough`,忽略)。

#### Scenario: 切换后下一轮用新 provider

- **WHEN** 对 `Agent` 调 `set_provider(new)` 后发起一轮 `run`
- **THEN** 该轮的模型调用落在 `new` 上,旧 provider 不被调用

#### Scenario: 切换传播到 context strategy

- **WHEN** Agent 装配了 `Compacting` 策略,随后 `set_provider(new)` / `set_model("m2")`
- **THEN** 此后由该策略触发的自动压缩调用落在 `new` provider / 用模型 `"m2"`(不再用旧 provider / 旧 model)

#### Scenario: Passthrough 策略忽略切换

- **WHEN** Agent 用 `Passthrough` 策略,调 `set_provider(new)`
- **THEN** 策略行为不变(原样返回 history),不报错

### Requirement: Plan 模式编排(mode-aware schema + 系统指令 + 纵深拒)

`Agent` SHALL 经 setter `set_permission_mode(Arc<Mutex<PermissionMode>>)`接入一个运行时可变的 `PermissionMode` 共享源(克隆自 TUI 侧共享状态)；字段默认 `Arc::new(Mutex::new(Normal))`,headless 默认 Normal，既有 `Agent::new` 签名与调用行为不变。

每轮循环顶部 MUST 读取一次 mode 快照并随即释锁；该轮 schema 装配、指令注入及本轮 tool_call 循环里的每一次纵深拒 MUST 复用同一个快照,不得在处理每个 tool_call 时重读 mutex:

- **mode-aware schema**:`ModelRequest.tools` 经 `registry.schemas_for(mode)` 取得。Plan 期下发 `ReadOnly + Network + plan_only`,摘掉 `Edit / Execute`。
- **plan 系统指令**:`mode==Plan` 时 MUST 把一条 plan 模式指令注入该轮 transient 请求 messages(`strategy.prepare` 产出的 Vec、进入 ModelRequest 前),MUST NOT 入持久 history。指令语义为:用户只是问 → 直接答；撞歧义 / 岔路 → `ask_user`；要执行任务 → `submit_plan` 提交结构化 plan，且每一步 MUST 带可独立验收的 `validation`；本地研究只读,web 研究工具可用但每次会请求 Network 授权,不得编辑文件或执行命令。非 Plan 不注入。
- **纵深拒(双向)**:① `mode==Plan` 且工具为 `Edit` / `Execute` → 直接产出 is_error ToolResult,不执行、不弹权限 UI；`Network` MUST NOT 命中该拒绝,而是进入 gate / decider。② `mode!=Plan` 且工具 `plan_only` → is_error 拒,循环继续。
- **快照封住中途翻转**:同一批 `[submit_plan, edit_file]` 中,submit_plan 批准即使把共享 mode 翻为 `AcceptEdits`,edit_file 仍按轮顶 Plan 快照被拒；翻转只影响下一轮。

对不含 Network tool_call 的非 Plan 既有脚本,run / run_observed 行为 MUST 与本 change 前一致。Network 在 Normal / AcceptEdits 下新增询问、在 Yolo 下自动放行,属于刻意的安全行为变化。mode 源、schema、指令、纵深拒与 Network gate 均须 headless Mock 可测。

#### Scenario: Plan 模式只下发 ReadOnly + Network + plan_only

- **WHEN** mode 源置 `Plan`,registry 含 ReadOnly / Network / Edit / Execute / plan_only 工具,跑一轮(Mock provider)
- **THEN** 该轮 `ModelRequest.tools` 仅含 ReadOnly / Network / plan_only 项(Edit/Execute 被摘),顺序保持

#### Scenario: Plan 指令保留 validation 并加入联网授权语义

- **WHEN** mode==`Plan` 跑一轮
- **THEN** 从 Mock provider 实收的首条 System message 直接断言其含问答 / ask_user / submit_plan 三分支、每步可验收 `validation`、web research 每次需 Network 授权及禁止 Edit / Execute；expected 不得引用 `PLAN_MODE_INSTRUCTION` 常量自身；该指令不入 history，Normal 不注入

#### Scenario: Plan 期 Network Allow 后执行

- **WHEN** mode==`Plan`,模型发出一个提供 `authorizable=true` 专用 preview 的 Network tool_call,decider 返回 Allow
- **THEN** 该调用不被纵深拒,工具执行并把正常 ToolResult 入 history,循环续跑

#### Scenario: Plan 期 Network Deny 零网络并续跑

- **WHEN** mode==`Plan`,模型发出 Network tool_call,decider 返回 Deny
- **THEN** 工具不执行、WebFetcher 零调用、is_error ToolResult 入 history,循环续跑

#### Scenario: 未知 Network 工具在 Agent Loop 中 fail-closed

- **WHEN** mode 为 Normal / Yolo / Plan 任一,模型调用未 override preview 的 Network tool,decider 返回 Allow
- **THEN** gate 最终 `Deny(NetworkUnauthorizable(reason))`、工具不 execute、带 reason 的 is_error ToolResult 入 history供模型修正,循环进入下一轮；不得写成 user denied

#### Scenario: Plan 期越界变更工具被纵深拒

- **WHEN** mode==`Plan`,模型发出一个 `Edit` / `Execute` 工具的 tool_call
- **THEN** 产出 is_error ToolResult(plan 拒变更)入 history、工具不执行、不发权限 UI,循环续跑

#### Scenario: 同批 submit_plan + 变更工具,快照封住中途翻转

- **WHEN** mode==`Plan`,模型在**一条回复**里发 `[submit_plan, edit_file]` 两个 tool_call;submit_plan 批准在本轮 tool 循环中途把共享 mode 翻 `AcceptEdits`
- **THEN** edit_file 仍按轮顶 Plan 快照被纵深拒；翻转仅令下一轮新快照为 AcceptEdits

#### Scenario: 非 Plan 模式硬发 plan_only 工具被拒

- **WHEN** mode!=`Plan`(如 `Normal`),模型硬发一个 `plan_only` 工具(如 `submit_plan`)的 tool_call
- **THEN** 产出 is_error ToolResult、工具不执行

#### Scenario: 非 Plan 的旧三类工具零回归

- **WHEN** mode==Normal 跑一个不含 Network tool_call 的既有脚本
- **THEN** history / 终止 / 错误 / 事件与本 change 前一致；schema 仍含 ReadOnly / Edit / Execute 且顺序不变

### Requirement: 每轮注入思考深度并回传思考块

`Agent` SHALL 提供 `set_thinking_depth(Arc<Mutex<Depth>>)` setter(字段默认 `Depth::Low`,MUST NOT 改 `Agent::new` 签名,仿 `set_permission_mode`)。`run_observed` 内**两处** `ModelRequest`(主循环 loop 顶、与循环外的 forced-final 收尾请求)SHALL 各读一次 depth 快照并填入 `ModelRequest.thinking = Some(ThinkingConfig{depth})`(forced-final 块在 for 外,MUST 重新读快照、MUST NOT 复用已出作用域的循环内局部变量;`Depth::Off` 也传,交由 wire 层按能力判 `None`/`disabled`)。**两处** `Message::Assistant` push(主循环与 forced-final 后)SHALL 各带上其轮 `response.thinking`,使下一轮请求经 wire 原样回传(满足 Anthropic 带 tool_use 多轮约束)。开启思考时 agent-loop MUST NOT 设置强制 `tool_choice`。`/model` 切换(`set_model`/`SetModel` 路径)SHALL 清空 caller `history` 内所有 `Message::Assistant.thinking`,以免跨模型 signature 在带 tool_use 多轮回传触发 400。

#### Scenario: depth 快照进请求

- **WHEN** 共享 depth 设为 `High`,agent 发起一轮
- **THEN** 该轮 `ModelRequest.thinking == Some(ThinkingConfig{High})`

#### Scenario: 思考块入 history 并下轮原样回传

- **WHEN** mock provider 返回带 `thinking`(text+signature)的 `ModelResponse`,随后有 tool_call 触发次轮
- **THEN** `Message::Assistant.thinking` 存该块;次轮请求序列化把该块字节一致回传

#### Scenario: 触顶 forced-final 轮也带 depth

- **WHEN** 共享 depth 设为 `High`,mock 触发 `max_iterations` 触顶走 forced-final 收尾请求
- **THEN** 该收尾 `ModelRequest.thinking == Some(ThinkingConfig{High})`(重新读快照,非默认/Off)

#### Scenario: /model 切换剥离旧思考块

- **WHEN** `history` 含带 `signature` 的 `Message::Assistant.thinking`,执行 `/model` 切换
- **THEN** 切换后 history 内所有 `Message::Assistant.thinking` 为空 `Vec`、其余字段不变

#### Scenario: 默认档零回归

- **WHEN** 未调 `set_thinking_depth`(默认 `Low`),既有 agent-loop 测试运行
- **THEN** 仅请求多带一个被 mock 忽略的 thinking 字段,既有断言保持绿、`Agent::new` 签名未变

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

### Requirement: Agent Loop 提供 scoped run 入口并保持 legacy 兼容

`Agent` SHALL 提供显式接收 execution scope 的 scoped run / observed run 入口。既有 `run` / `run_observed` MUST 作为兼容 wrapper，为每次调用创建无 deadline、无 child capability 扩张入口的 root scope，并委托同一 scoped 实现；除 observer 新增的 run identity 外，未取消 legacy 调用的 history、schema、Provider 请求、tool 调度、返回值与错误分流 MUST 保持不变。

#### Scenario: scoped 与 legacy 正常完成结果等价
- **WHEN** 用相同 Mock Provider 脚本分别调用 legacy 与等价 root scoped 入口且均不取消
- **THEN** 两次的 Provider 请求、history、最终文本及 tool outcome 逐字段一致

#### Scenario: legacy wrapper 不复用上一轮 scope
- **WHEN** 同一个 Agent 先后执行两个 legacy run
- **THEN** 两次各自使用新的 root identity，前一轮 cancellation state 不影响后一轮

### Requirement: cancellation 在 Agent Loop 内确定性收口

scoped run MUST 在 context preparation、每次 Provider 请求、permission decision、串行 tool execute、并行批次等待及 forced-final 请求处同时等待 scope termination。若 cancellation 或 deadline 在当前 `User` 之后、任何 `Assistant` 提交之前发生，Loop MUST 从模型 history 回滚该未提交的当前 `User` turn，使下一轮 Provider 请求不再携带旧任务；TUI transcript MAY 保留用户输入与 interrupted 展示。若 termination 在某条 `Assistant.tool_calls` 写入 history 后发生，Loop MUST 保留当前 `User`、该 `Assistant` 与已按 occurrence 发布的结果，并为该 Assistant 中每个尚未发布的 occurrence 按模型顺序追加且仅追加一个 is_error `ToolResult`；canceled 与 deadline-exceeded MUST 使用可区分的稳定内容。尚未启动的工具不得启动，已启动但未发布的 future 必须被 drop，其迟到结果不得进入 history 或 observer；已进入 blocking pool 的无副作用工作 MAY 自然结束，但结果仍必须丢弃。cancellation只约束Agent编排、future与结果发布，MUST NOT宣称回滚已发生的工具副作用或保证终止已由工具启动的OS进程。收口后 scoped run MUST 返回独立 `ScopedAgentError::{Cancelled,DeadlineExceeded}`（普通Agent错误以另一个variant包装），不得给既有公开`AgentError`增variant，不得请求下一轮 Provider或进入 forced-final。

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

### Requirement: Agent dispatch统一使用当前scoped工具上下文

Agent Loop SHALL 在串行与ParallelSafe dispatch中为每个tool occurrence构造仅借用既有`ToolContext`、当前`AgentExecutionScope`、当前`AgentObserver`和Agent可选read root的`ToolExecutionContext`，并统一调用tool的scoped执行入口。上下文 MUST 逐future传递且不得写入共享Tool可变状态；两个并发run使用同一registry时不得串scope、observer或read root。

#### Scenario: legacy Tool default转发零回归
- **WHEN** 只实现既有`execute`的Tool分别从串行和ParallelSafe路径执行
- **THEN** scoped default方法恰调用一次原`execute`，args、ToolContext、ToolOutcome、observer顺序与变更前逐字段一致

#### Scenario: 并发run不串scoped上下文
- **WHEN** 两个root scope并发通过同一Tool实例执行，且scope identity、observer与read root均不同
- **THEN** 每个future只观察到自己的四项上下文，取消其中一个不会改变另一个

#### Scenario: child depth不足时schema与dispatch双重拒绝
- **WHEN** scope的remaining child depth小于Tool声明的required child depth
- **THEN** 该Tool不出现在Provider schema；模型硬发时在observer started、permission gate与execute前得到scope error

### Requirement: delegate作为普通outer occurrence参与Loop收口

Agent Loop SHALL 把`delegate_task`的成功或child-only失败作为普通ToolOutcome按既有occurrence规则写入parent history；parent cancellation/deadline仍由outer scope termination优先裁决并生成synthetic结果。串行路径在tool future返回后、调用history/finished observer发布前 MUST 再次检查parent scope。ParallelSafe路径 MAY 把乱序完成项暂存于仅内部可见的ready buffer，但每个item进入连续可发布前缀、即将同步写history/finished observer且中间无`await`时 MUST 再次检查parent scope；只有这次紧邻发布的post-ready checkpoint SHALL 作为该occurrence的publication linearization point。观察到termination时，当前及所有尚未发布的ready outcome均被丢弃并进入synthetic收口；通过后才允许同步发布。连续ParallelSafe delegate calls沿用上限4、ready buffer与模型顺序发布；child future不得绕过permission/mode/unknown-tool屏障或直接写parent history。

#### Scenario: child失败后parent可继续
- **WHEN** child Provider失败或child deadline到达而parent scope仍可运行
- **THEN** parent history加入一个is_error delegate ToolResult并带完整history请求下一轮Provider，不返回全局ScopedAgentError

#### Scenario: parent终止覆盖未发布child结果
- **WHEN** child物理结果ready，但post-ready checkpoint观察到parent cancellation或deadline
- **THEN** ready结果不得写入history或finished observer；若它已乱序暂存于私有ready buffer则必须丢弃，该occurrence按既有termination文案收口且child不得产生迟到finished/usage/Idle

#### Scenario: nested cancellation不能抢先发布ordinary error
- **WHEN** outer termination branch首次poll为Pending，而delegate future随后在同一次poll中观察parent-derived cancellation并返回child cancellation error或其他ready outcome
- **THEN** post-ready checkpoint必须把该结果提升为outer termination，禁止发布`delegate_task failed:`或普通finished事件

#### Scenario: 私有ready buffer不提前线性化
- **WHEN** ParallelSafe批次的后项先ready并进入私有buffer、前项仍未完成，此时parent终止且后项尚未写入history/finished observer
- **THEN** 后项不得因较早ready而视为已发布；它与前项及其余未发布occurrence都按outer synthetic termination收口，不发布普通outcome

#### Scenario: delegate批次保持模型顺序
- **WHEN** 多个delegate futures乱序完成
- **THEN** outer results、observer finished及下一轮Provider messages仍按原occurrence发布，全部结果完成前不得请求下一轮Provider

#### Scenario: child forced-final observer序列完整
- **WHEN** child触及iteration上限并在forced-final Provider响应中返回`usage: Some`
- **THEN** 同一child identity在该请求前收到`CallingModel`、响应后恰收到一次对应usage callback，成功自然终止再收到`Idle`；ChannelObserver可过滤child status但不得漏掉该usage，Provider error、termination或空final失败路径不得发送`Idle`
