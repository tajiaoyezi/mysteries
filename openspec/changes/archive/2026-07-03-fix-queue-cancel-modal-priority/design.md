# Design — fix-queue-cancel-modal-priority

## D1 —— 让位规则(不变量:浮层活跃时行为 == 无排队基线)

`handle_queue_cancel_key` 的早退条件从 `pending_permission.is_some()` 扩为:

```
!is_key_press || pending_permission.is_some()
    || models_picker.is_some() || command_completion.is_some()
    || !has_queue()
```

让位后键落到既有链路:picker 经 `apply_batch_input_key` 硬模态分支自消费(Esc 关闭),补全经 `on_key` 的 `handle_command_completion_key`(Esc 关浮层)。**不清排队、不投 Interrupt、不记 `last_cancel_at`**。`should_exit` 的排队让位块不动(它返回 false 只是把键放行给后续处理,浮层会消费,无退出风险)。

弃「整批截断/在 queue-cancel 内代答浮层」:浮层已有完整路由,复刻即漂移。

## D2 —— 推进闸门可测化(等价重构)

ui_rx 臂现内联「算 `is_terminal` → `apply_ui_event` → 推进」三步,依赖 `select!` 作用域不可测。抽:

```
fn handle_agent_event(state, event, calling_model_started_at, first_token_at, input_tx)
```

内部保持原三步(`is_terminal` 在 apply 前算,推进 = `is_terminal && has_queue` 时 `dequeue_next` + `send(Prompt)`,同一调用内)。`select!` 臂只调此函数。四个集成测试直接驱动它(3.4 原定口径):

1. TurnComplete + 排队 ["B","C"] → 恰发一条 `Prompt("B")`(channel ≤1)、transcript 尾 `User("B")`、phase Busy、"C" 仍在队;
2. Error 收场 → 仍推进;
3. Interrupted → 仍推进;run_agent_task 侧既有中断测试补「Interrupted 后短窗内无任何尾随事件」断言(锁"不再紧跟 Idle");
4. `StatusChanged(Idle)` → 不推进、phase 不变;Idle 后提交 → 入队不直发;随后 TurnComplete → 恰发一条。

## D3 —— 记录更正

log 39 末尾追加更正注(比照 b4f429d 更正 log 40 的先例);新 log 41 记本 change 决策与 rejected。归档 tasks.md(2026-07-03-add-message-queue)不改写——历史归档保持原样,以 log 更正为准。
