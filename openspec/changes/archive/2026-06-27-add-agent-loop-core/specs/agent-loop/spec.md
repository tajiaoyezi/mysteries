## ADDED Requirements

### Requirement: 多轮编排循环

系统 SHALL 提供 Agent Loop:从初始 history(System + User)出发,每轮以**完整 history** 请求 provider,将回复的 text 与 tool_calls 落为一条 `Assistant` 消息入 history;若该回复无 tool_calls,循环 SHALL 终止并返回最终回复文本;若有 tool_calls,则逐个处理(权限门 + 执行),将每个结果作为 `ToolResult` 入 history 后,带累积 history 再请求。6 类事件(用户输入、模型文本、工具调用、工具结果、权限拒绝、错误)MUST 全部映射进 history 的 `Message`(§5.5)。

#### Scenario: 无 tool_calls 单轮终止

- **WHEN** provider 首个回复不含 tool_calls
- **THEN** 循环返回该回复文本,history 末尾为对应 `Assistant` 消息,且不再发起请求

#### Scenario: 含工具的多轮编排

- **WHEN** provider 第一轮返回一个 tool_call、第二轮返回无 tool_call 的文本
- **THEN** 依次发生:`Assistant{tool_calls}` 入 history → 工具结果 `ToolResult` 入 history → 带累积 history 再请求 → `Assistant{text}` 入 history 并终止;且第二次请求携带的 history 包含第一轮的全部消息

### Requirement: max_iterations 守卫

循环 MUST 受 `max_iterations` 限制;达到上限仍未自然终止时 SHALL 以致命错误 `AgentError::MaxIterations` 终止,不得无限循环。

#### Scenario: 触顶致命终止

- **WHEN** provider 每轮都返回 tool_call(永不自然终止)且 `max_iterations = N`
- **THEN** 第 N 轮后循环以 `AgentError::MaxIterations` 终止,不再发起请求

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
