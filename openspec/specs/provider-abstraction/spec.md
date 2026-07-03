# provider-abstraction Specification

## Purpose
provider-abstraction 是内核与 LLM 后端之间的归一化边界:内核仅依赖 `Provider` trait(async、dyn 安全)与 `ModelRequest` / `ModelResponse` / `ProviderError` 等归一化类型,不感知任何具体线格式;本域同时约定 OpenAI 兼容线格式的序列化 / 解析规则、`ProviderError` 的可恢复 / 致命分类、token 用量透传,以及驱动内核测试的 `MockProvider`。设计立场是向两端解耦:流式增量经 `DeltaSink` 出口而非直写 UI,凭据以构造时注入凭据名、`complete` 内经 `CredentialChain` 解析,使逻辑 provider 身份与 wire 协议族(kind)正交。边界:凭据来源与链属 credential-source,provider 到模型目录的映射属 provider-registry,「何种具体条件映射到何错误变体」留给各传输实现。
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

### Requirement: ProviderError 可恢复 / 致命错误分类

`ProviderError` SHALL 增补三个变体表达 §9 的「可恢复 vs 致命」语义,与既有 `Transport` / `Decode` 并列:`Auth`(鉴权失败,**致命**,不重试)、`RateLimited`(限流,**可重试**)、`Timeout`(超时,**可重试**)。新增变体 MUST 保持 `ProviderError` 既有的 `PartialEq` / `Eq` 派生(供测试断言);其语义为传输层重试策略提供判定依据 —— 致命变体终止、可重试变体进入退避重试。「何种具体条件 → 何变体」的映射属各传输实现(见 `openai-transport`),不在本抽象层固化。

> 背景:bootstrap design D9 预告了 `Auth` / `RateLimited` / `Timeout` 「要在有真实调用时才有构造点」;`add-openai-live-transport` 即其构造点。既有 `Transport`(§9)/ `Decode`(归一化解析失败)语义不变。

#### Scenario: 致命变体终止、可重试变体退避重试

- **WHEN** 传输层产出 `ProviderError::Auth`
- **THEN** 重试策略将其判为致命、不重试,直接上抛

#### Scenario: 可重试变体进入重试

- **WHEN** 传输层产出 `ProviderError::RateLimited` 或 `ProviderError::Timeout`
- **THEN** 重试策略将其判为可重试,触发指数退避重试(至上限后方上抛)

### Requirement: 响应携带 token 用量

`ModelResponse` SHALL 携带 `usage: Option<Usage>`,暴露该轮真实 token 用量;`Usage` MUST 含 `input_tokens` 与 `output_tokens`(均 `u32`),并以方法 `total()` 给出二者之和,MUST NOT 存独立 total 字段(避免与 `input + output` 不一致)。当 provider 未回传可用用量(端点不支持 / 字段缺失 / Mock 未设)时,`usage` MUST 为 `None`,MUST NOT 臆造 `0` 或 panic。`Usage` 解析失败 MUST 降级为 `None`、MUST NOT 使 `complete` 失败(用量为辅助计量;既有 text / tool_calls 的 `Decode` 致命语义不变)。`ModelResponse` 与 `FinishReason` SHALL 派生 `Default`(`FinishReason::default()` = `Stop`),使既有与未来构造点可经 `..Default::default()` 兜未显式字段。

#### Scenario: usage 经 ModelResponse 透传

- **WHEN** 一个带 `usage: Some(Usage{ input_tokens, output_tokens })` 的 `ModelResponse` 由 provider 产出
- **THEN** 调用方可从 `ModelResponse.usage` 读到该 `Usage`,且 `total()` 等于 `input_tokens + output_tokens`

#### Scenario: 无用量为 None

- **WHEN** provider 响应不含可用 token 用量
- **THEN** `ModelResponse.usage` 为 `None`,`complete` 仍正常返回完整 text / tool_calls / finish_reason

#### Scenario: MockProvider 可设用量

- **WHEN** 以脚本 `[ModelResponse{ usage: Some(..), .. }]` 构造 Mock 并调用 `complete`
- **THEN** 返回的 `ModelResponse` 携带该预设 `usage`(Mock 无需新增 API,经预置响应透传)

### Requirement: Provider 凭据名构造注入

真实 provider 实现(`OpenAiProvider` / `AnthropicProvider`)SHALL 支持在**构造时注入用于解析 API key 的「凭据名」**,并在 `complete` 中用该名经 `CredentialChain` resolve 密钥,而非固定使用其 wire kind 的默认名。这使「逻辑 provider 身份」(凭据键)与「wire 协议族」(kind)解耦——例如一个 `kind=OpenAi` 的 provider 可用凭据名 `deepseek` 解析,与 `openai` 键分离。**未注入凭据名的既有构造路径**(`new` / `default` / 既有带 timeout 构造器)MUST 回落到 kind 默认名(`OpenAi`→`"openai"`、`Anthropic`→`"anthropic"`),使既有 provider 行为(含「凭据缺失 → `ProviderError::Auth`」)**逐字节不变**。凭据名注入 MUST NOT 改变 `Provider` trait 签名、MUST NOT 触网;凭据解析失败仍在 `complete` 内、HTTP 之前以 `ProviderError::Auth` fail-fast(凭据为辅助前置,非网络期)。

#### Scenario: 注入凭据名后按该名解析(离线)

- **WHEN** 以注入凭据名 `"deepseek"` 构造一个 `kind=OpenAi` 的 provider,其 `CredentialChain` 仅含 `"openai"` 键(不含 `"deepseek"`),调用 `complete`
- **THEN** 返回 `ProviderError::Auth`(按注入名 `"deepseek"` 解析未命中,**未**回落误用 `"openai"`),且解析在 HTTP 之前、不触网

#### Scenario: 未注入凭据名回落 kind 默认名(零回归)

- **WHEN** 以既有默认构造路径(不注入凭据名)构造 `OpenAiProvider`,其 `CredentialChain` 为空
- **THEN** 按 kind 默认名 `"openai"` 解析未命中 → `ProviderError::Auth`,与本 change 前行为一致(既有 provider 单测保持绿)

#### Scenario: 注入凭据名命中则不因 Auth 失败(离线)

- **WHEN** 以注入凭据名 `"deepseek"` 构造 provider,其 `CredentialChain` 含 `"deepseek"` 键
- **THEN** `complete` 的凭据前置解析命中(不返回 `ProviderError::Auth`),即按注入名而非 kind 名取密钥;构造期不触网

