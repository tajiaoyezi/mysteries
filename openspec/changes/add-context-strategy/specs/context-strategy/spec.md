## ADDED Requirements

### Requirement: 请求前上下文策略钩子

系统 SHALL 提供 `ContextStrategy`(async、`Send + Sync`、dyn 安全),方法 `prepare(&self, history: &[Message]) -> Result<Vec<Message>, ContextError>`:`Agent` 在**每轮请求前**经它由 history 产出实际发送给 provider 的 messages。系统 SHALL 提供默认实现 `Passthrough`,其 `prepare` 原样返回 history(逐条等价),使未注入策略时 `Agent` 行为与无策略时**逐字节一致**。`Agent` SHALL 默认装配 `Passthrough`,并提供注入替换策略的入口(供后续压缩实现接入)。trait MUST 为 async,以支持后续需 `await` 的实现(如调用 provider 生成 summary);本能力仅建立钩子,MUST NOT 含任何压缩 / 截断逻辑。

#### Scenario: Passthrough 逐条等价

- **WHEN** 以任意 `Vec<Message>` 调 `Passthrough::prepare`
- **THEN** 返回的 `Vec<Message>` 与输入 history 逐条相等(顺序与内容一致)

#### Scenario: 默认装配零回归

- **WHEN** 用默认(未注入策略)的 `Agent` 跑既有 agent-loop 各场景
- **THEN** 请求所携 messages 与接线前一致,循环 / 终止 / 错误 / 事件行为不变(既有 agent-loop 测试保持绿)

#### Scenario: 可注入替换策略

- **WHEN** 向 `Agent` 注入一个非 `Passthrough` 的 `ContextStrategy`
- **THEN** 此后每轮请求前由该策略的 `prepare` 决定发送的 messages
