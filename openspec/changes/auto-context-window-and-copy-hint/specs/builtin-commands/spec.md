# builtin-commands Delta

## MODIFIED Requirements

### Requirement: /compact 手动压缩

`/compact` 命令 SHALL 立即对当前会话 history 跑一次压缩(**无视阈值**,直接压),复用与自动压缩**同一** `Compacting` 逻辑(被压区间 / 结构化 summary / 入 `System` / 保留窗口与正确性红线一致)。压缩默认启用(有效窗口按「上下文窗口解析」求得,见 context-strategy),MUST NOT 出现「压缩未启用」类提示,`run_compact_command` MUST 以非 `Option` 的压缩句柄执行(「未启用」分支删除)。

**发起门控**:`/compact` 仅在 `phase == Ready` 且无排队时可发起——发起即置 `Phase::Compacting` 并 send `UserInput::Compact`;运行中 / 有排队时 SHALL 拒绝并回 notice(如「当前有任务进行中,/compact 请稍后再试」),MUST NOT 把 `Compact` 排进 channel 延迟执行。

**进行中(动画与排队)**:`Compacting` 期间 activity line SHALL 以 spinner 显示「压缩上下文…」(accent 样式,**不**提示 esc 中断——压缩不可中断,v1 Non-Goal);phase 非 Ready,期间提交按「消息排队」(tui-shell)语义入可见队列。

**收场**:压缩结束(成功或失败)agent task SHALL 依次发 `Notice(结果文案)` 与 `AgentEvent::CompactDone`;`apply(CompactDone)` 置 `phase = Ready`,且 `CompactDone` 计入排队推进闸门(见 tui-shell「消息排队」)。成功 notice MUST 为**不含消息数**的简短文案(「已压缩上下文」);summary 失败时 SHALL 回 notice 提示可重试(history 不变),MUST NOT panic。命令解析与执行走既有 builtin-commands 语义(同 `/model` 等)。

#### Scenario: /compact 立即压缩且 notice 不含计数

- **WHEN** 就绪且无排队时输入 `/compact`(Mock provider 返回 summary)
- **THEN** 当前 history 被替换为 `[ System(原 system + summary), 最近 keep_recent_turns 轮 ]`;成功 notice 为「已压缩上下文」(**不含**前后消息数);随后 `CompactDone` 置回 Ready

#### Scenario: /compact summary 失败回 notice(仍收场)

- **WHEN** 输入 `/compact` 但 summary 的 `provider.complete` 失败
- **THEN** history 保持不变,回一条 notice 提示压缩失败 / 可重试,不 panic;**仍发 `CompactDone`** 置回 Ready(不卡在 Compacting)

#### Scenario: 未配 model_context_window 时 /compact 仍可压

- **WHEN** 未配 `model_context_window` 时输入 `/compact`(Mock provider 返回 summary)
- **THEN** 照常压缩(手动压无视阈值,与窗口无关),全程不出现「压缩未启用」类提示、不 panic

#### Scenario: 运行中 / 有排队时 /compact 被拒

- **WHEN** `phase` 运行中(如 `CallingModel`),或 `phase == Ready` 但 `pending_queue` 非空,输入 `/compact`
- **THEN** **不** send `UserInput::Compact`、phase 不变,transcript 追加一条「稍后再试」notice

#### Scenario: 压缩进行中活动行动画(insta 快照)

- **WHEN** `phase == Compacting`,`TestBackend` 渲染
- **THEN** activity line 为 spinner + 「压缩上下文…」(accent 样式、**无** esc 中断提示),与锁定快照一致
