## ADDED Requirements

### Requirement: 请求携带工具定义

`ModelRequest` SHALL 携带工具定义列表(`tools`);OpenAI 请求序列化在**有工具时** SHALL 输出 `tools` 数组,每项形如 `{type:"function", function:{name, description, parameters}}`;**无工具时** MUST NOT 输出 `tools` 键(与既有消息序列化行为保持兼容)。

> 背景:change 1 按其 design D5 暂省略了 `ModelRequest.tools`,约定「工具 change 补回」;本 change 即补回项。既有 System/User/Assistant/ToolResult 的序列化行为不变。

#### Scenario: 有工具时序列化 tools 数组

- **WHEN** 序列化一个带两个工具定义的 `ModelRequest`
- **THEN** 请求体含 `tools` 数组,两项各含 `function.name` / `function.description` / `function.parameters`

#### Scenario: 无工具时不输出 tools 键

- **WHEN** 序列化一个不带任何工具定义的 `ModelRequest`
- **THEN** 请求体不含 `tools` 键(与 change 1 行为一致)
