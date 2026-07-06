# openai-transport Specification

## Purpose
定义 OpenAI Chat Completions 协议的接入传输层:`OpenAiProvider` 的 HTTP 请求与 Bearer 鉴权、SSE 流式累积(文本增量即时经 `DeltaSink` 推出、tool_calls 按 index 拼接、usage 经 `stream_options.include_usage` 解析),以及 per-attempt 超时与指数退避重试。关键立场是 IO 与逻辑分离:SSE 累积器与「HTTP 结果 → `ProviderError` / 是否可重试」的分类均为与 reqwest 解耦的纯逻辑,可离线单测;`base_url` 可配,同一实现覆盖官方与 OpenAI 兼容端点(如 DeepSeek / 本地网关)。`Provider` trait 与凭据链属 provider-abstraction / credential-source;本域的 retry / timeout 传输逻辑同时被 anthropic-transport 复用。
## Requirements
### Requirement: OpenAiProvider 实 HTTP 请求

系统 SHALL 提供 `OpenAiProvider`,impl `Provider`,持可配 `base_url`(默认 `https://api.openai.com/v1`,亦支持本地 / 自定义兼容端点)。`complete` MUST 以 `wire::serialize_request` 归一化请求体,在传输层附加 `"stream": true`,并 POST 到 `{base_url}/chat/completions`。请求所用模型 MUST 取自 `ModelRequest.model`(不由 provider 自持),与既有 `wire` 序列化 / `Agent` 装配一致。

#### Scenario: 请求体携带归一化 messages 与 stream 标志

- **WHEN** 以一个 `ModelRequest` 构造 OpenAI chat 请求(离线,不发网络)
- **THEN** 请求体含 `wire::serialize_request` 产出的归一化 `messages`(及 `model` = `req.model`),且含 `"stream": true`;目标 URL 为 `{base_url}/chat/completions`

### Requirement: 鉴权与凭据缺失致命

`OpenAiProvider` MUST 从注入的 `CredentialChain` 以 `resolve("openai")` 取密钥,经 `expose_secret()` 构造 `Authorization: Bearer <key>` 头;明文 MUST NOT 出现在错误信息 / 日志(仅 `expose_secret()` 处解封)。当 `resolve("openai")` 返回 `None`,`complete` MUST 立即返回 `ProviderError::Auth`(§9 致命),且 MUST NOT 发起任何 HTTP 请求、MUST NOT 进入重试。

#### Scenario: 凭据缺失立即致命且不触网

- **WHEN** `CredentialChain` 对 `"openai"` 解析为 `None`(如空链 / 注入的空 env lookup),调用 `complete`
- **THEN** 返回 `Err(ProviderError::Auth)`,且未发起任何 HTTP 请求、未重试

#### Scenario: 携带 Bearer 鉴权头

- **WHEN** `CredentialChain` 对 `"openai"` 命中密钥,构造请求(离线)
- **THEN** 请求带 `Authorization: Bearer <key>`,`<key>` 经 `expose_secret()` 取得;密钥明文不进入任何错误 / 日志输出

### Requirement: SSE 流式累积

系统 SHALL 提供一个**与 reqwest 解耦的纯累积逻辑**(吃字节 chunk),按 OpenAI SSE 解析 `data:` 事件:文本增量(`choices[].delta.content`)MUST 经 `DeltaSink` 即时推出;工具调用增量(`choices[].delta.tool_calls[]`,带 `index`)MUST 按 `index` 累积其 `id` / `name` / `arguments` 片段;`[DONE]` / 流结束 MUST 落成 `ModelResponse`。每个 `tool_call` 的 `arguments` 片段拼接后 MUST 解析为 `serde_json::Value`,非法 → `ProviderError::Decode`。累积器 MUST 正确处理跨 chunk 边界被切断的 SSE 事件(不丢、不重)。

#### Scenario: 文本增量即时推送、tool_call 按 index 累积、[DONE] 落成响应

- **WHEN** 把一段 fixture SSE 字节流(含多个文本 `delta`、若干 `tool_calls` 分片、`[DONE]`)喂给累积器
- **THEN** `DeltaSink` 按到达顺序收到文本增量;最终 `ModelResponse` 的 `text` / `tool_calls`(`arguments` 已解析为 `Value`)/ `finish_reason` 均正确

#### Scenario: 跨 chunk 边界缝合

- **WHEN** fixture 字节流在任意位置(含一个 SSE 事件中段)被切成多个 chunk 依次喂入
- **THEN** 累积结果与不切分时一致(累积器内部缓冲跨 chunk 缝合,不丢不重)

#### Scenario: 非法 tool_call arguments

- **WHEN** 某 `tool_call` 各 `arguments` 片段拼接后不是合法 JSON
- **THEN** 累积落成时返回 `ProviderError::Decode`(不 panic)

### Requirement: 超时与指数退避重试

`OpenAiProvider` MUST 以 per-attempt `tokio::time::timeout` 包裹每次请求尝试;超时 → `ProviderError::Timeout`。对 `429` / `5xx` / 网络错误 / `Timeout` MUST 以指数退避重试至上限;对 `Auth` 及其他致命错误(非可重试 4xx、`Decode`)MUST 立即失败不重试。重试耗尽 MUST 返回最后一次错误(对 Agent loop 即致命)。「何种 HTTP 结果 → 何 `ProviderError` 变体 / 是否可重试」MUST 由一个**与 reqwest 解耦的纯分类逻辑**决定(吃 status `u16` / 抽象传输错误 kind),以便离线单测。

#### Scenario: 限流 / 服务端错误触发重试

- **WHEN** 注入的尝试连续返回可重试结果(如 `429` → `RateLimited`)再返回成功
- **THEN** 经指数退避重试后返回成功的 `ModelResponse`,尝试次数 = 失败次数 + 1

#### Scenario: 401 鉴权失败不重试

- **WHEN** 注入的尝试返回 `401`(分类为 `Auth`)
- **THEN** 立即返回 `Err(ProviderError::Auth)`,只尝试一次、不重试

#### Scenario: 403 forbidden 不重试且非 Auth

- **WHEN** 注入的尝试返回 `403`
- **THEN** 立即返回 fatal `ProviderError::Transport`,message 含 `forbidden (403)` 及模型/配额提示;**不**映射为 `Auth`;只尝试一次、不重试

#### Scenario: 重试耗尽返回最后错误

- **WHEN** 注入的尝试在重试上限内始终返回可重试错误
- **THEN** 达上限后返回最后一次的 `ProviderError`(`RateLimited` / `Timeout` / 可重试 `Transport`)

#### Scenario: 单次尝试超时记为 Timeout

- **WHEN** 一次尝试耗时超过 per-attempt 超时预算(测试用虚拟时间推进)
- **THEN** 该尝试被记为 `ProviderError::Timeout` 并触发重试

### Requirement: OpenAI token 用量解析

`OpenAiProvider` 的请求体 SHALL 附加 `stream_options: { include_usage: true }`(OpenAI 流式默认不回 usage),与既有 `stream: true` 并存。SSE 累积逻辑 MUST 识别流末尾 `choices` 为空、携带 `usage` 的 usage-only chunk,取其 `prompt_tokens` → `input_tokens`、`completion_tokens` → `output_tokens` 填入 `ModelResponse.usage`;该 usage-only chunk MUST NOT 被误当文本 / 工具增量。usage 缺失 / 解析失败 MUST 降级为 `usage = None`,MUST NOT 影响既有 text / tool_calls / finish_reason 的累积。本 requirement 为叠加,既有「SSE 流式累积」「OpenAiProvider 实 HTTP 请求」requirement 的行为不变。

#### Scenario: 含 usage chunk 的流解析出 usage

- **WHEN** 把含末尾 usage-only chunk(`prompt_tokens` / `completion_tokens`)的 OpenAI SSE 字节流喂给累积器
- **THEN** 最终 `ModelResponse.usage` = `Some(Usage{ input_tokens = prompt_tokens, output_tokens = completion_tokens })`,且 text / tool_calls / finish_reason 与无 usage chunk 时一致

#### Scenario: 请求开启 include_usage

- **WHEN** 以一个 `ModelRequest` 构造 OpenAI chat 请求(离线,不发网络)
- **THEN** 请求体含 `stream_options.include_usage = true`,与既有 `stream: true` 并存

#### Scenario: 无 usage chunk 降级为 None

- **WHEN** 流中不含 usage-only chunk(端点不支持 `include_usage`)
- **THEN** `ModelResponse.usage` 为 `None`,其余字段照常累积、`complete` 不失败

### Requirement: OpenAI reasoning_effort 映射与 reasoning 模型参数适配

`wire`(OpenAI 兼容)的请求序列化 SHALL 依 `openai_thinking_capability(model)` 与请求 `Depth` 映射,**`max_completion_tokens` 与 `reasoning_effort` 解耦**(前者是 reasoning 模型属性、与是否开思考无关):
- 能力 `Effort`(reasoning 模型)→ 输出上限字段 MUST **恒**用 `max_completion_tokens`(不论 `Depth`,含 `Off`;reasoning 模型在 Chat Completions 上见 `max_tokens` 会 400);
- 能力 `Effort` 且 `Depth≠Off` → **额外**发顶层 `reasoning_effort=<depth capped>`;
- 能力 `None` → 不动 body(不发 `reasoning_effort`、保持 `max_tokens`)。
Assistant 分支 MUST NOT 回传 `signature`(OpenAI 不回传推理正文;`ThinkingBlock.signature` 对 OpenAI 恒 `None`)。`stream` MAY 解析兼容网关的 `delta.reasoning_content` 并调 `on_thinking`;OpenAI 官方无该字段时思考展示留空。

#### Scenario: reasoning 模型开思考改用 max_completion_tokens

- **WHEN** model=`gpt-5`(Effort)、`Depth::Medium`、`max_tokens=Some(4096)`,序列化请求
- **THEN** body 含顶层 `reasoning_effort="medium"` 与 `max_completion_tokens=4096`,且**不含** `max_tokens`

#### Scenario: reasoning 模型 Off 仍用 max_completion_tokens

- **WHEN** model=`gpt-5`(Effort)、`Depth::Off`、`max_tokens=Some(4096)`
- **THEN** body 用 `max_completion_tokens=4096`(不含 `max_tokens`)、**不含** `reasoning_effort`(`/think off` 不使 reasoning 模型 400)

#### Scenario: 非 reasoning 模型不发思考字段

- **WHEN** model 未知(能力 `None`)、任意 `Depth`
- **THEN** body 不含 `reasoning_effort`、仍用 `max_tokens`(与引入前一致)

#### Scenario: 兼容网关 reasoning_content 流式外发

- **WHEN** stream 收到含 `delta.reasoning_content` 的分片
- **THEN** `on_thinking` 被调;分片不含该字段时不调、不报错

