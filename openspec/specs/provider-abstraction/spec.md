# provider-abstraction Specification

## Purpose
TBD - created by archiving change bootstrap-provider-core. Update Purpose after archive.
## Requirements
### Requirement: 归一化的 Provider 接口

系统 SHALL 通过 `Provider` trait 暴露 LLM 后端。该 trait MUST 为 async 且 dyn 安全(`Box<dyn Provider>` 可用),内核 MUST 仅依赖归一化类型 `ModelRequest` / `ModelResponse` / `ProviderError`,不依赖任何具体线格式或 UI 类型。

#### Scenario: 经 trait object 调用

- **WHEN** 调用方持有 `Box<dyn Provider>` 并以一个 `ModelRequest` 调用 `complete`
- **THEN** 返回 `Result<ModelResponse, ProviderError>`,且调用方无需知道具体 provider 类型

### Requirement: 流式输出经 DeltaSink 解耦

Provider 产出的增量文本 SHALL 经 `DeltaSink` 出口推出,而非直接写 stdout 或 UI;Provider MUST NOT 依赖任何 UI channel。增量去向(stdout / 测试捕获 / no-op)由调用方决定。

#### Scenario: 文本增量推送到 sink

- **WHEN** provider 在 `complete` 期间产出一段或多段文本增量
- **THEN** 每段增量经 `DeltaSink::on_text` 推出,由调用方提供的 sink 接收

#### Scenario: no-op sink 不影响结果

- **WHEN** 调用方传入一个 no-op `DeltaSink`
- **THEN** `complete` 仍正常返回完整的 `ModelResponse`,增量被静默丢弃

### Requirement: OpenAI 请求归一化(序列化)

系统 SHALL 将内核 `Message` 序列化为 OpenAI 兼容请求体的 messages 数组:`System` → `role:"system"`;`User` → `role:"user"`;`Assistant{text, tool_calls}` → `role:"assistant"`(携带 `tool_calls[]`);`ToolResult{call_id, content}` → `role:"tool"`(携带 `tool_call_id`)。

#### Scenario: 四类消息映射到正确 role

- **WHEN** 一组依次含 System / User / Assistant(带 tool_calls)/ ToolResult 的会话被序列化
- **THEN** 产出的 messages 数组各项 `role` 与字段符合 OpenAI 约定,且 `ToolResult` 项的 `tool_call_id` 正确回填为对应调用 id

### Requirement: OpenAI 响应归一化(解析)

系统 SHALL 将 OpenAI 非流式响应体解析为 `ModelResponse`:取 `choices[0].message` 的文本与 `tool_calls`;OpenAI 以 JSON 字符串承载的 `function.arguments` MUST 被解析为 `serde_json::Value`;`finish_reason` MUST 映射为 `FinishReason`。解析失败 MUST 返回 `ProviderError`,不得 panic。

#### Scenario: 纯文本响应

- **WHEN** 解析一个仅含 assistant 文本、`finish_reason:"stop"` 的响应体
- **THEN** 得到 `ModelResponse{ text = 该文本, tool_calls = [], finish_reason = Stop }`

#### Scenario: 含 tool_calls 的响应

- **WHEN** 解析一个 `finish_reason:"tool_calls"`、且 `tool_calls[].function.arguments` 为 JSON 字符串的响应体
- **THEN** 得到的 `tool_calls` 已归一化(`id` / `name` 正确,`arguments` 为解析后的 `serde_json::Value`),`finish_reason = ToolCalls`

#### Scenario: 非法响应体

- **WHEN** 解析一个缺少 `choices` 或结构非法的响应体
- **THEN** 返回 `ProviderError`(不 panic)

### Requirement: MockProvider 脚本化与请求记录

系统 SHALL 提供 `MockProvider`:按预置 `Vec<ModelResponse>` 顺序逐次返回,记录每次收到的 `ModelRequest` 供测试断言,并经 `DeltaSink` 吐出该轮回复文本的增量。脚本耗尽后再调用 MUST 返回 `ProviderError`(fail-safe),不得 panic。

#### Scenario: 按脚本顺序返回

- **WHEN** 以脚本 `[R0, R1]` 构造 Mock 并连续调用 `complete` 两次
- **THEN** 依次返回 `R0`、`R1`

#### Scenario: 记录收到的请求

- **WHEN** 以某 `ModelRequest` 调用 Mock
- **THEN** 该请求被记录,测试可取出并断言其 `model` 与 `messages`

#### Scenario: 脚本耗尽

- **WHEN** 脚本已用尽仍调用 `complete`
- **THEN** 返回 `ProviderError`,不 panic

### Requirement: 请求携带工具定义

`ModelRequest` SHALL 携带工具定义列表(`tools`);OpenAI 请求序列化在**有工具时** SHALL 输出 `tools` 数组,每项形如 `{type:"function", function:{name, description, parameters}}`;**无工具时** MUST NOT 输出 `tools` 键(与既有消息序列化行为保持兼容)。

> 背景:change 1 按其 design D5 暂省略了 `ModelRequest.tools`,约定「工具 change 补回」;本 change 即补回项。既有 System/User/Assistant/ToolResult 的序列化行为不变。

#### Scenario: 有工具时序列化 tools 数组

- **WHEN** 序列化一个带两个工具定义的 `ModelRequest`
- **THEN** 请求体含 `tools` 数组,两项各含 `function.name` / `function.description` / `function.parameters`

#### Scenario: 无工具时不输出 tools 键

- **WHEN** 序列化一个不带任何工具定义的 `ModelRequest`
- **THEN** 请求体不含 `tools` 键(与 change 1 行为一致)

