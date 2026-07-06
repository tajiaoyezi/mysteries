# openai-transport Delta

## ADDED Requirements

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
