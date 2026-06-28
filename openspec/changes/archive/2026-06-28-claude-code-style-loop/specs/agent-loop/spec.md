## MODIFIED Requirements

### Requirement: max_iterations 守卫

循环 MUST 受 `max_iterations` 限制(高位**安全网**,默认 50,仍可经配置覆盖),不得无限循环。循环跑满 `max_iterations` 轮仍未自然终止时,SHALL **不**直接以 `AgentError::MaxIterations` 终止,而是**追加一次** `provider.complete`、该次 `ModelRequest.tools` 传**空**(禁用工具),强制模型基于现有 history 产出文字回答:该次有文字则其 `Assistant{text}` 入 history 并返回 `Ok(text)`;仅当该次仍无文字(空 text 且无可用 tool_calls)时,才以致命错误 `AgentError::MaxIterations` 终止。强制收尾那次 `provider.complete` 自身返回 `Err` 时,按既有「provider 错误致命」分流为 `AgentError::Provider`。

#### Scenario: 触顶强制收尾产出文字

- **WHEN** provider 前 N 轮都返回 tool_call(永不自然终止)且 `max_iterations = N`,第 N+1 次调用(tools 已禁用)返回不含 tool_call 的文本
- **THEN** 第 N+1 次请求的 `ModelRequest.tools` 为空,其文本作为 `Assistant{text}` 入 history,循环返回 `Ok(text)`,不再发起请求

#### Scenario: 强制收尾仍无文字才致命兜底

- **WHEN** 跑满 `max_iterations` 轮后,强制收尾那次(tools 禁用)仍未产出文字
- **THEN** 循环以 `AgentError::MaxIterations` 终止

## ADDED Requirements

### Requirement: system prompt 身份约束

`DEFAULT_SYSTEM_PROMPT` SHALL 含身份约束:禁止冒充 Claude / ChatGPT / OpenAI / Anthropic 或任何具体上游模型;被问及模型身份时,只说明运行于 Mysteries、所配置的模型名见状态行。该约束 MUST 由单测锁定关键短语(存在即绿,缺失即红)。

#### Scenario: 默认 system prompt 含身份约束短语

- **WHEN** 取 `DEFAULT_SYSTEM_PROMPT`
- **THEN** 其文本含 `Do not claim to be Claude`、`ChatGPT`、`OpenAI`、`Anthropic` 与「模型名见状态行」对应短语(`configured model name is shown in the status line`),任一缺失使单测失败
