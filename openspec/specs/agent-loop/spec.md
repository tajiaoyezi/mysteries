# agent-loop Specification

## Purpose
TBD - created by archiving change add-agent-loop-core. Update Purpose after archive.
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

系统 SHALL 提供 `AgentObserver`(`Send + Sync`,方法 `on_status` / `on_tool_call_started` / `on_tool_call_finished` / `on_usage`,**全部 default no-op**)与 `AgentStatus`(`Idle` / `CallingModel` / `ExecutingTool(String)` / `WaitingForPermission`),以及 `Agent::run_observed(history, ctx, sink, observer)`:在循环关键点经 observer 发结构化事件 —— 模型调用前 `StatusChanged(CallingModel)`;工具分发时 `StatusChanged(ExecutingTool(name))` 与 `on_tool_call_started{id, name, args, readonly}`(`readonly` 取自工具 `permission_level`);`RequiresConfirmation` 工具询问前 `WaitingForPermission`;工具产出结果后 `on_tool_call_finished{id, outcome}`(执行结果 / 拒绝 / 未知工具均以 `ToolOutcome` 上报);循环自然终止前 `Idle`。每次 `provider.complete` 返回后,若 `ModelResponse.usage` 为 `Some`,MUST 经 `observer.on_usage(&usage)` 上送该轮真实 token 用量;`usage` 为 `None` 的轮 MUST NOT 上送。`on_usage` 取 `&Usage`(`provider-abstraction` 已定义),**default no-op**。

既有 `Agent::run` 的契约(history 累积、终止条件、错误分流,见本能力既有 requirement)MUST 保持不变,且 `run` MUST 委托 `run_observed` 并传入 no-op observer —— `run` 的行为与本 change 前**逐字节一致**(`on_usage` default no-op 故不影响 `run`)。`AgentObserver` 方法的 default no-op MUST 使任何不关心观测的调用方零负担。

#### Scenario: 观测一轮工具调用的事件顺序

- **WHEN** 以 Mock 脚本「轮1 → 一个工具的 tool_call、轮2 → 终复文本」调用 `run_observed`,传入一个记录事件的 observer
- **THEN** observer 依次收到 `CallingModel` → `ToolCallStarted` →(`ExecutingTool` / 必要时 `WaitingForPermission`)→ `ToolCallFinished` → 下一轮 `CallingModel` → `Idle`,且 history 与 `run` 等价(`on_usage` 是否触发取决于脚本是否带 usage,不改变上述 status / tool 事件的相对顺序)

#### Scenario: run 委托后行为与原一致(零回归)

- **WHEN** 调用既有 `Agent::run`(不带 observer)跑任意脚本
- **THEN** 其 history、返回值、终止 / 错误行为与本 change 前完全一致(`run` 委托 `run_observed` + no-op observer,既有 agent-loop 测试保持绿)

#### Scenario: 权限拒绝仍上报工具完成

- **WHEN** 某 `RequiresConfirmation` 工具被 decider 拒绝
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

