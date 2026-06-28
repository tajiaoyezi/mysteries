## ADDED Requirements

### Requirement: /compact 手动压缩

`/compact` 命令 SHALL 立即对当前会话 history 跑一次压缩(**无视阈值**,直接压),复用与自动压缩**同一** `Compacting` 逻辑(被压区间 / 结构化 summary / 入 `System` / 保留窗口与正确性红线一致)。压缩结果替换会话 history,并回一条 notice(含压缩前后消息数);summary 失败时 SHALL 回 notice 提示可重试(history 不变),MUST NOT panic。压缩禁用(未配 `model_context_window`)或无 provider 时,`/compact` SHALL 回提示而非压缩、MUST NOT panic。命令解析与执行走既有 builtin-commands 语义(同 `/model` 等)。

#### Scenario: /compact 立即压缩

- **WHEN** 在配了 `model_context_window` 的会话中输入 `/compact`(Mock provider 返回 summary)
- **THEN** 当前 history 被替换为 `[ System(原 system + summary), 最近 keep_recent_turns 轮 ]`,回一条 notice 含压缩前后消息数

#### Scenario: /compact summary 失败回 notice

- **WHEN** 输入 `/compact` 但 summary 的 `provider.complete` 失败
- **THEN** history 保持不变,回一条 notice 提示压缩失败 / 可重试,不 panic

#### Scenario: 压缩禁用时 /compact 回提示

- **WHEN** 未配 `model_context_window` 时输入 `/compact`
- **THEN** 回一条提示(压缩未启用 / 需配 `model_context_window`),history 不变、不 panic
