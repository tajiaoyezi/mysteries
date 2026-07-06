# anthropic-transport Delta

## ADDED Requirements

### Requirement: Anthropic 思考请求映射、流式解析与多轮原样回传

`anthropic_wire` 的请求序列化 SHALL 依 `anthropic_thinking_capability(model)` 与请求 `Depth` 映射思考字段:
- `Adaptive` 且 `Depth≠Off` → 顶层 `thinking={type:"adaptive", display:"summarized"}` + **独立顶层** `output_config={effort: <depth capped>}`;
- `Adaptive` 且 `Depth=Off` 且 `can_disable` → `thinking={type:"disabled"}`;`Depth=Off` 且 `!can_disable`(恒开模型)→ 仅 `output_config={effort:"low"}`、不发 thinking;
- `Budget` 且 `Depth≠Off` → `thinking={type:"enabled", budget_tokens: clamp(max_tokens×ratio, 1024, max_tokens-1), display:"summarized"}`(`budget_tokens` MUST `< max_tokens` 且 `≥1024`);**`max_tokens` 为 `None` 或 `<1025` 时 MUST 不发 budget_tokens**(退回省略 thinking),以免 `clamp(_,1024,max_tokens-1)` 在 `min>max` 时 u32 panic;`Depth=Off` → 省略 thinking;
- `None` → 不动 body。
系统 MUST NOT 设置 `tool_choice` 强制工具(any/tool)以免与思考不兼容(默认 auto)。

`serialize_request` 的 Assistant 分支 SHALL 把该消息 `thinking: Vec<ThinkingBlock>` 的每块作为 content 数组**首批**元素、排在 text/tool_use **之前**、**逐字节原样**回传:`{type:"thinking", thinking:<text>, signature:<sig>}`,`redacted` 块作 `{type:"redacted_thinking", data:<sig>}`;思考载体为空则维持引入前的 content 结构。此为带 tool_use 多轮不被 400 拒的硬约束。

`anthropic_stream` SHALL 解析思考流:`content_block_start` 识别 `type=="thinking"|"redacted_thinking"` 建块;`content_block_delta` 处理 `thinking_delta`(累积 text 并调 `on_thinking` 流式外发)与 `signature_delta`(累积 signature);`finish` 把累积块保序推入 `ModelResponse.thinking`(含 `thinking` 为空文本的 omitted 块)。

#### Scenario: 当代模型 adaptive+effort 请求体

- **WHEN** model=`claude-opus-4-8`(Adaptive)、`Depth::Medium`,序列化请求
- **THEN** body 含 `thinking={type:"adaptive",display:"summarized"}` 与顶层 `output_config={effort:"medium"}`;不含 `budget_tokens`

#### Scenario: 老模型 budget 请求体且 budget<max_tokens

- **WHEN** model=`claude-haiku-4-5`(Budget)、`Depth::High`、`max_tokens=16000`
- **THEN** body 含 `thinking={type:"enabled",budget_tokens:N,display:"summarized"}` 且 `1024 ≤ N < 16000`

#### Scenario: max_tokens 过小/None 时 Budget 分支不发 budget_tokens

- **WHEN** model=`claude-haiku-4-5`(Budget)、`Depth::High`、`max_tokens=1000`(或 `None`)
- **THEN** 不 panic、body 省略 thinking(不发 budget_tokens)

#### Scenario: Off 分模型处理

- **WHEN** `Depth::Off` 对 `claude-sonnet-5`(can_disable) vs `claude-fable-5`(恒开)
- **THEN** 前者 `thinking={type:"disabled"}`;后者不发 thinking、发 `output_config={effort:"low"}`

#### Scenario: 带 tool_use 的 Assistant 原样回传 thinking 块

- **WHEN** 序列化一条 `thinking` 非空且含 `tool_calls` 的 `Message::Assistant`
- **THEN** content 数组首元素为 `{type:"thinking",thinking,signature}`(字节一致)、其后才是 tool_use;signature 未被改动

#### Scenario: 流式累积 thinking 与 signature

- **WHEN** 依次喂 `thinking_delta`(文本分片)+ `signature_delta` + `content_block_stop`
- **THEN** `on_thinking` 被逐片调用;`finish` 后 `ModelResponse.thinking` 含该块、text 拼全、signature 完整
