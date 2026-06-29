# context-strategy Specification

## Purpose
TBD - created by archiving change add-context-strategy. Update Purpose after archive.
## Requirements
### Requirement: 请求前上下文策略钩子

系统 SHALL 提供 `ContextStrategy`(async、`Send + Sync`、dyn 安全),方法 `prepare(&self, history: &[Message], last_usage: Option<&Usage>) -> Result<Vec<Message>, ContextError>`:`Agent` 在**每轮请求前**经它由 history 产出实际发送给 provider 的 messages,并传入**上一轮 response 的 `usage`** 作 `last_usage`(首轮 `None`),供需要 token 用量的策略(如压缩)判定。系统 SHALL 提供默认实现 `Passthrough`,其 `prepare` **忽略 `last_usage`**、原样返回 history(逐条等价),使未注入策略时 `Agent` 行为与无策略时**逐字节一致**。`Agent` SHALL 默认装配 `Passthrough`,并提供注入替换策略的入口(供压缩实现接入)。trait MUST 为 async,以支持需 `await` 的实现(如调用 provider 生成 summary);本钩子能力本身 MUST NOT 含任何压缩 / 截断逻辑(由具体策略实现)。

#### Scenario: Passthrough 逐条等价(忽略 last_usage)

- **WHEN** 以任意 `Vec<Message>` 与任意 `last_usage`(`Some` 或 `None`)调 `Passthrough::prepare`
- **THEN** 返回的 `Vec<Message>` 与输入 history 逐条相等(顺序与内容一致),与 `last_usage` 无关

#### Scenario: 默认装配零回归

- **WHEN** 用默认(未注入策略)的 `Agent` 跑既有 agent-loop 各场景
- **THEN** 请求所携 messages 与接线前一致,循环 / 终止 / 错误 / 事件行为不变(既有 agent-loop 测试保持绿)

#### Scenario: 可注入替换策略

- **WHEN** 向 `Agent` 注入一个非 `Passthrough` 的 `ContextStrategy`
- **THEN** 此后每轮请求前由该策略的 `prepare` 决定发送的 messages

#### Scenario: Agent 以上一轮 usage 作 last_usage 传入

- **WHEN** `Agent` 连续跑多轮,上一轮 `response` 带 `usage = Some(..)`
- **THEN** 下一轮 `prepare` 收到的 `last_usage` 为上一轮 `response.usage`;**首轮** `prepare` 的 `last_usage` 为 `None`

### Requirement: Compacting 压缩策略

系统 SHALL 提供 `Compacting`(impl `ContextStrategy`),据真实 token 用量在接近 context window 上限时把 history 压成结构化 summary。**触发条件**:`last_usage` 为 `Some` 且 `last_usage.input_tokens > model_context_window × compact_trigger_ratio`。触发时 `prepare` MUST 把 history 重写为 `[ System(原 system prompt 追加结构化 summary), <最近 keep_recent_turns 个完整轮原文> ]`;被压区间(`system` 之后到保留窗口之前)的 summary 由 `provider.complete`(`tools` 空)以**结构化 prompt** 生成(分节:已完成工作 / 当前文件与代码状态 / 关键决策 / 下一步待办)。**未触发**(`last_usage` 为 `None`,或未超阈值)时 MUST 原样返回 history(等价 `Passthrough`)。

约束(正确性红线):① summary MUST 拼入 `System` message、MUST NOT 引入额外独立 message —— 以保 `user`/`assistant` 交替(Anthropic 强制交替,连续同 role → `400`);② 保留窗口边界 MUST 对齐完整轮:从末尾向前数 `keep_recent_turns` 个 `User` 消息为界,界前压、界后(含该 `User`)原样留,MUST NOT 切断 `assistant.tool_calls` ↔ `tool_result` 配对;③ `system` 永留;④ summary 可被下次压缩再压(幂等)。

**降级**:summary 的 `provider.complete` 失败 MUST 退回不压(返回原 history、`Ok`),MUST NOT 致命终止本轮(自动路径下轮仍超阈值会再触发)。

#### Scenario: 超阈值触发结构化压缩

- **WHEN** `last_usage.input_tokens > window × ratio`,以含多轮的 history 调 `Compacting::prepare`(Mock provider 脚本返回一段 summary 文本)
- **THEN** 返回 history 为 `[ System(原 system + summary), 最近 keep_recent_turns 轮原文 ]`,summary 取自 provider 输出,消息数显著少于输入

#### Scenario: 未超阈值 / 无 usage 等价 Passthrough

- **WHEN** `last_usage` 为 `None`,或 `input_tokens ≤ window × ratio`
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

### Requirement: Compacting 运行时 provider / model 切换

`Compacting` SHALL 支持运行时替换其用于摘要的 provider(`set_provider(Arc<dyn Provider>)`)与 model(`set_model(String)`)。替换后,后续 `compact_now` / 自动压缩 MUST 经新 provider / model 发出摘要请求。`ContextStrategy` trait SHALL 暴露增量的默认 no-op `set_provider` / `set_model` 钩子,使非 `Compacting` 策略(如 `Passthrough`)默认忽略切换,`Compacting` override 之以更新自身字段。

#### Scenario: 切换后压缩走新 provider

- **WHEN** 对 `Compacting` 调 `set_provider(new)` + `set_model("m2")`,随后 `compact_now`
- **THEN** 摘要请求落在 `new` provider、用模型 `"m2"`

#### Scenario: Passthrough 默认钩子 no-op

- **WHEN** 对 `Passthrough` 经 `ContextStrategy` 钩子调 `set_provider` / `set_model`
- **THEN** 不报错且行为不变(无 provider 概念,默认实现为空)

