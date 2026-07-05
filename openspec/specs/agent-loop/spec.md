# agent-loop Specification

## Purpose
agent-loop 是内核的多轮编排中枢:每轮以完整 history 请求 Provider,经权限门与工具系统处理回复中的 tool_calls,并把全部事件(用户输入、模型文本、工具调用与结果、权限拒绝、错误)统一落为 history 中的 `Message`,直到模型产出无 tool_calls 的回复。设计立场是 history 为唯一事实源、错误按可恢复 / 致命分流(工具失败以 is_error 的 `ToolResult` 入 history 续跑,provider 错误致命上抛),`max_iterations` 触顶时先禁用工具追加一次强制收尾而非直接报错;观测(`AgentObserver`)与运行时 provider / model 切换均以 default no-op、不改 `run` 既有契约的方式增量提供。本域只负责编排次序、终止与错误分流:工具的定义与执行属 tool-system,放行决策属 permission-gate,每轮实际发送的 messages 由 context-strategy 产出。
## Requirements
### Requirement: 多轮编排循环

系统 SHALL 提供 Agent Loop:从初始 history(System + User)出发,每轮以**完整 history** 请求 provider,将回复的 text 与 tool_calls 落为一条 `Assistant` 消息入 history;若该回复无 tool_calls,循环 SHALL 终止并返回最终回复文本;若有 tool_calls,则逐个处理(权限门 + 执行),将每个结果作为 `ToolResult` 入 history 后,带累积 history 再请求。6 类事件(用户输入、模型文本、工具调用、工具结果、权限拒绝、错误)MUST 全部映射进 history 的 `Message`(§5.5)。

#### Scenario: 无 tool_calls 单轮终止

- **WHEN** provider 首个回复不含 tool_calls
- **THEN** 循环返回该回复文本,history 末尾为对应 `Assistant` 消息,且不再发起请求

#### Scenario: 含工具的多轮编排

- **WHEN** provider 第一轮返回一个 tool_call、第二轮返回无 tool_call 的文本
- **THEN** 依次发生:`Assistant{tool_calls}` 入 history → 工具结果 `ToolResult` 入 history → 带累积 history 再请求 → `Assistant{text}` 入 history 并终止;且第二次请求携带的 history 包含第一轮的全部消息

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

系统 SHALL 提供 `AgentObserver`(`Send + Sync`,方法 `on_status` / `on_tool_call_started` / `on_tool_call_finished` / `on_usage`,**全部 default no-op**)与 `AgentStatus`(`Idle` / `CallingModel` / `ExecutingTool(String)` / `WaitingForPermission`),以及 `Agent::run_observed(history, ctx, sink, observer)`:在循环关键点经 observer 发结构化事件 —— 模型调用前 `StatusChanged(CallingModel)`;工具分发时 `StatusChanged(ExecutingTool(name))` 与 `on_tool_call_started{id, name, args, readonly}`(`readonly` 取自工具 `permission_level`);非 `ReadOnly`(`Edit` / `Execute`)工具询问前 `WaitingForPermission`;工具产出结果后 `on_tool_call_finished{id, outcome}`(执行结果 / 拒绝 / 未知工具均以 `ToolOutcome` 上报);循环自然终止前 `Idle`。每次 `provider.complete` 返回后,若 `ModelResponse.usage` 为 `Some`,MUST 经 `observer.on_usage(&usage)` 上送该轮真实 token 用量;`usage` 为 `None` 的轮 MUST NOT 上送。`on_usage` 取 `&Usage`(`provider-abstraction` 已定义),**default no-op**。

既有 `Agent::run` 的契约(history 累积、终止条件、错误分流,见本能力既有 requirement)MUST 保持不变,且 `run` MUST 委托 `run_observed` 并传入 no-op observer —— `run` 的行为与本 change 前**逐字节一致**(`on_usage` default no-op 故不影响 `run`)。`AgentObserver` 方法的 default no-op MUST 使任何不关心观测的调用方零负担。

#### Scenario: 观测一轮工具调用的事件顺序

- **WHEN** 以 Mock 脚本「轮1 → 一个工具的 tool_call、轮2 → 终复文本」调用 `run_observed`,传入一个记录事件的 observer
- **THEN** observer 依次收到 `CallingModel` → `ToolCallStarted` →(`ExecutingTool` / 必要时 `WaitingForPermission`)→ `ToolCallFinished` → 下一轮 `CallingModel` → `Idle`,且 history 与 `run` 等价(`on_usage` 是否触发取决于脚本是否带 usage,不改变上述 status / tool 事件的相对顺序)

#### Scenario: run 委托后行为与原一致(零回归)

- **WHEN** 调用既有 `Agent::run`(不带 observer)跑任意脚本
- **THEN** 其 history、返回值、终止 / 错误行为与本 change 前完全一致(`run` 委托 `run_observed` + no-op observer,既有 agent-loop 测试保持绿)

#### Scenario: 权限拒绝仍上报工具完成

- **WHEN** 某非 `ReadOnly`(`Edit` / `Execute`)工具被 decider 拒绝
- **THEN** observer 收到 `WaitingForPermission` 后,该工具以 is_error 的 `ToolOutcome`(user denied)触发 `on_tool_call_finished`,且既有「denial 入 history、循环继续」行为不变

#### Scenario: 每轮 model 调用后上送 token 用量

- **WHEN** 以 Mock 脚本(其 `ModelResponse` 带 `usage: Some(Usage{..})`)调用 `run_observed`,传入记录事件的 observer
- **THEN** 该次 model 调用返回后 observer 收到 `on_usage` 携该轮 `Usage`;若某轮 `ModelResponse.usage` 为 `None` 则该轮不收到 `on_usage`;`run`(no-op observer)行为与本 change 前逐字节一致

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

`Agent` SHALL 经 **setter** `set_permission_mode(Arc<Mutex<PermissionMode>>)`(仿 `set_strategy`,**不改 `Agent::new` 签名**;字段默认 `Arc::new(Mutex::new(Normal))`,故既有 `Agent::new` 调用与行为逐字节不变)接入一个**运行时可变的 `PermissionMode` 共享源**(克隆自 TUI 侧共享状态;headless 默认 `Normal`)。

每轮循环 **顶部读取一次 mode 快照**(`let mode = *lock` 单次、随即释锁),该轮的 schema 装配、指令注入、**及本轮 tool_call 循环里的每一次纵深拒 MUST 复用这同一个快照**,MUST NOT 在处理每个 tool_call 时重读 mutex:
- **mode-aware schema**:`ModelRequest.tools` 用 `registry.schemas_for(mode)` 取代 `registry.schemas()` —— `Plan` 期只下发只读 + plan_only 工具(schema-omit,见 tool-system)。
- **plan 系统指令**:`mode==Plan` 时 MUST 把一条 plan 模式 system 指令注入**该轮的 transient 请求 messages**(即 `strategy.prepare` 产出的 `Vec<Message>`、`ModelRequest` 之前;**MUST NOT** 入持久 `history`,否则逐轮累积并被存进 session 快照);语义三分支:**用户只是问 → 直接答;撞歧义/岔路 → 用 `ask_user` 弹选项让用户定;要执行任务 → 用 `submit_plan` 交带每步 `validation` 的结构化 plan**;只读、不改文件/不执行命令。非 Plan 不注入。
- **纵深拒(用快照,双向)**:① `mode==Plan` 且某 tool_call 的工具**非 `ReadOnly`**(schema-omit 之外的越界)→ MUST 直接产出 is_error 的 `ToolResult`、**不执行、不弹权限 UI**;② `mode!=Plan` 且某 tool_call 的工具 `plan_only`(如凭记忆硬发 `submit_plan`)→ 亦 MUST is_error 拒(对称防御)。二者 schema-omit 为主控制、纵深拒为辅,循环续跑。
- **快照封住中途翻转**:同一批 `[submit_plan, edit_file]` 中,`submit_plan` 批准会**在本轮 tool 循环中途**把共享 mode 翻 `Plan→AcceptEdits`;因纵深拒复用**轮顶快照(仍 Plan)**,`edit_file` 兄弟仍被拒;翻转只影响**下一轮**的新快照(届时全工具可用、执行已批准 plan)。

既有 `run` / `run_observed` 在非 Plan(默认 `Normal`)下行为 MUST 与本 change 前**逐字节一致**(`schemas_for(Normal)` 对未注册 plan_only 工具的 registry 与 `schemas()` 保序逐字节相等)。mode 源、注入、纵深拒逻辑 headless 可测(setter 注固定 mode 源 + Mock provider)。

#### Scenario: Plan 模式只下发只读 + plan_only

- **WHEN** mode 源置 `Plan`,registry 含只读 / 变更 / plan_only 工具,跑一轮(Mock provider)
- **THEN** 该轮 `ModelRequest.tools` 仅含只读 + plan_only 项(变更类被摘)

#### Scenario: Plan 模式注入三分支系统指令

- **WHEN** mode==`Plan` 跑一轮
- **THEN** 该轮 messages 含一条 plan 模式 system 指令(问答 / ask_user / submit_plan 三分支);mode==`Normal` 时该指令不出现

#### Scenario: Plan 期越界变更工具被纵深拒

- **WHEN** mode==`Plan`,模型发出一个 `Edit` / `Execute` 工具的 tool_call
- **THEN** 产出 is_error 的 `ToolResult`(plan 拒变更)入 history、工具不执行、不发权限 UI,循环续跑

#### Scenario: 同批 submit_plan + 变更工具,快照封住中途翻转

- **WHEN** mode==`Plan`,模型在**一条回复**里发 `[submit_plan, edit_file]` 两个 tool_call;submit_plan 批准在本轮 tool 循环中途把共享 mode 翻 `AcceptEdits`
- **THEN** `edit_file` 兄弟仍按**轮顶快照(Plan)**被纵深拒(不执行、未静默放行);翻转仅令**下一轮**新快照为 `AcceptEdits`

#### Scenario: 非 Plan 模式硬发 plan_only 工具被拒

- **WHEN** mode!=`Plan`(如 `Normal`),模型硬发一个 `plan_only` 工具(如 `submit_plan`)的 tool_call
- **THEN** 产出 is_error 的 `ToolResult`(对称防御)、工具不执行

#### Scenario: 非 Plan 零回归

- **WHEN** mode==`Normal`(默认)跑任意既有脚本
- **THEN** history / 终止 / 错误 / 事件与本 change 前一致(`schemas_for(Normal)` 等价既有;无 plan_only 工具时逐字节一致)

