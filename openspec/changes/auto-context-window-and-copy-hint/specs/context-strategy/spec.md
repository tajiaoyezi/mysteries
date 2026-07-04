# context-strategy Delta

## ADDED Requirements

### Requirement: 上下文窗口解析(显式配置 > 内置表 > 保守默认)

系统 SHALL 提供纯函数 `context_window_for(model: &str) -> Option<u32>`(内置模型窗口表)与 `resolve_context_window(explicit: Option<u32>, model: &str) -> u32`,解析优先级 MUST 为:显式配置 `model_context_window` > 内置表匹配 > 保守默认 `DEFAULT_CONTEXT_WINDOW = 65_536`(取小不取大:估小仅致压缩偏早,估大会致压缩缺席)。

内置表 MUST:大小写不敏感;**顺序敏感**,更特定条目在前、首个命中生效(如 `gpt-4.1` / `gpt-4o` / `gpt-4-turbo` 不被 `gpt-4` 遮蔽);长 pattern(> 2 字符)按子串匹配以容忍网关前缀名(如 `openai/gpt-4o`),短 pattern(`o1` / `o3` / `o4`)按边界匹配(全等 / `{p}-` 起头 / 含 `/{p}`)防误伤。

`Compacting` 的触发判定 MUST 在**判定时**以**当前 model** 解析有效窗口(而非构造时固定),使 `/model`、`/models` 运行时切换后窗口自动跟随新 model。解析 MUST 为纯逻辑、无 IO,可离线单测。

#### Scenario: 解析优先级

- **WHEN** `resolve_context_window(Some(50_000), "claude-sonnet-4")`;`resolve_context_window(None, "claude-sonnet-4")`;`resolve_context_window(None, "totally-unknown-model")`
- **THEN** 依次得 `50_000`(显式覆盖优先)、`200_000`(内置表)、`65_536`(保守默认)

#### Scenario: 表匹配大小写不敏感、特定优先、边界防误伤

- **WHEN** 解析 `"Claude-Sonnet-4"`、`"gpt-4.1-mini"`、`"openai/gpt-4o"`、`"o3-mini"`、`"yi-o1-chat"`(均无显式配置)
- **THEN** 分别命中 claude、gpt-4.1(不被 gpt-4 遮蔽)、gpt-4o(网关前缀可匹配)、o3;`"yi-o1-chat"` **不**命中 o1(边界匹配),走保守默认

#### Scenario: 切 model 后窗口跟随

- **WHEN** `Compacting`(无显式覆盖)以表内大窗口 model 构造,同一 `last_usage` 不触发压缩;`set_model` 切到表内小窗口 model 后再判定
- **THEN** 同一 `last_usage` 在新 model 下触发压缩(有效窗口按当前 model 实时解析)

## MODIFIED Requirements

### Requirement: Compacting 压缩策略

系统 SHALL 提供 `Compacting`(impl `ContextStrategy`),据真实 token 用量在接近 context window 上限时把 history 压成结构化 summary。**触发条件**:`last_usage` 为 `Some` 且 `last_usage.input_tokens > 有效窗口 × compact_trigger_ratio`,其中**有效窗口**按「上下文窗口解析」以当前 model 于判定时求得(显式配置 > 内置表 > 保守默认)。触发时 `prepare` MUST 把 history 重写为 `[ System(原 system prompt 追加结构化 summary), <最近 keep_recent_turns 个完整轮原文> ]`;被压区间(`system` 之后到保留窗口之前)的 summary 由 `provider.complete`(`tools` 空)以**结构化 prompt** 生成(分节:已完成工作 / 当前文件与代码状态 / 关键决策 / 下一步待办)。**未触发**(`last_usage` 为 `None`,或未超阈值)时 MUST 原样返回 history(等价 `Passthrough`)。

约束(正确性红线):① summary MUST 拼入 `System` message、MUST NOT 引入额外独立 message —— 以保 `user`/`assistant` 交替(Anthropic 强制交替,连续同 role → `400`);② 保留窗口边界 MUST 对齐完整轮:从末尾向前数 `keep_recent_turns` 个 `User` 消息为界,界前压、界后(含该 `User`)原样留,MUST NOT 切断 `assistant.tool_calls` ↔ `tool_result` 配对;③ `system` 永留;④ summary 可被下次压缩再压(幂等)。

**降级**:summary 的 `provider.complete` 失败 MUST 退回不压(返回原 history、`Ok`),MUST NOT 致命终止本轮(自动路径下轮仍超阈值会再触发)。

#### Scenario: 超阈值触发结构化压缩

- **WHEN** `last_usage.input_tokens > 有效窗口 × ratio`,以含多轮的 history 调 `Compacting::prepare`(Mock provider 脚本返回一段 summary 文本)
- **THEN** 返回 history 为 `[ System(原 system + summary), 最近 keep_recent_turns 轮原文 ]`,summary 取自 provider 输出,消息数显著少于输入

#### Scenario: 未超阈值 / 无 usage 等价 Passthrough

- **WHEN** `last_usage` 为 `None`,或 `input_tokens ≤ 有效窗口 × ratio`
- **THEN** `prepare` 原样返回 history(逐条等价),不发起 summary 请求

#### Scenario: 保留窗口对齐完整轮、不切 tool 配对

- **WHEN** 被压区间与保留窗口的边界恰落在某轮的 `assistant.tool_calls` 与其 `tool_result` 之间附近
- **THEN** 边界被对齐到 `User` 处,重写后的 messages 中每个 `assistant.tool_calls` 仍与其全部 `tool_result` 同侧(保留或被压),不出现悬空 tool_call

#### Scenario: summary 入 System 不新增独立 message

- **WHEN** 触发压缩
- **THEN** 重写后**不含**为 summary 单独新增的 `User` / `Assistant` message,summary 仅作为 `System` 内容的一部分;`system` 后续消息满足 user/assistant 交替

#### Scenario: summary 失败退回不压

- **WHEN** 触发压缩,但 summary 的 `provider.complete` 返回 `Err`
- **THEN** `prepare` 返回原 history(`Ok`),不致命终止;本轮不压
