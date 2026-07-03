# 2026-07-03 · 41 · archive fix-queue-cancel-modal-priority

## 决策
- **队列取消让位浮层:`handle_queue_cancel_key` 早退条件扩入 `models_picker.is_some() || command_completion.is_some()`** | 选:让位后键走既有模态/浮层路由,行为 == 无排队基线(不投 Interrupt、不记 `last_cancel_at`、不清队) | 弃:在 queue-cancel 内代答浮层(复刻既有路由必漂移)、整批截断(picker 是多键过滤面,burst-submit change 已论证过) | 主导:主 agent 事后审查 CONFIRMED-2(有排队时开 `/models` 或输 `/` 后按 Esc → 当前轮被误中断,600ms 内再按整队被静默清空) | 依据:code(优先级链修正为 pending > 选区 > 硬模态/软浮层 > 有排队两级取消 > 运行中中断 > 就绪退出;red-green,红灯即 bug 复现)
- **推进闸门可测化:ui_rx 臂内联三步抽 `handle_agent_event`(等价重构,`select!` 臂单行调用)** | 为 add-message-queue 3.4 补课——归档 tasks.md 误勾 `[x]` 而四个集成测试实未写,推进闸门(`is_terminal && has_queue → dequeue+send`)零覆盖 | 弃:测试自建组合 helper 绕开真实路径(测的不是产线组合) | 主导:主 agent 事后审查 CONFIRMED-1 | 依据:tests(4 集成测试直接驱动 `handle_agent_event`;mutation 验证:禁用推进块 4/4 FAILED)
- **「Interrupted 后无尾随事件」补断言**:既有 run_agent_task 中断测试在 Interrupted 后加 80ms timeout 断言无任何尾随事件——add-message-queue 只是删掉了旧测试对 Idle 的期望,没断言其不出现 | 依据:tests

## 变更
- `src/tui/mod.rs`:`handle_queue_cancel_key` 加两浮层让位条件;ui_rx 臂抽 `fn handle_agent_event(state, event, calling_model_started_at, first_token_at, input_tx)`;新增 6 测(2 让位 red-green + 4 推进集成)+ 中断测试补尾随断言。
- spec:`tui-shell`「运行中可中断」MODIFIED——分流链加入硬模态/软浮层让位,优先级链修正,新增 Scenario「有排队时浮层的 Esc 不被取消排队劫持」;「中断收场」Scenario 加尾随断言要求。
- log 39 原地追加更正注(两项审查发现)。
- `cargo test --lib` 475 passed / 0 failed / 2 ignored;clippy 零警告;快照零 churn;`openspec validate --strict` 过。

## 待决
- 有排队 + 浮层开着时按 Esc 关浮层后,**再**按 Esc 才进入两级取消——阈值窗口从关浮层那次不计时(让位不记 `last_cancel_at`),手感待真机确认。
- 审查遗留的次要观察(未修,记录在案):`PASTE_COALESCE_MIN_EVENTS=4` 下按住键连发可能持续续批(D8 已接受类);`SetText`/历史召回不 `prune_pasted`(纯卫生,无功能影响)。

## 引用
- OpenSpec change:`fix-queue-cancel-modal-priority` → archive/2026-07-03-fix-queue-cancel-modal-priority
- 源头:[[2026-07-03-39-archive-add-message-queue]](被修复/补课对象)、[[2026-07-03-40-archive-add-paste-fold]](次要观察涉及其阈值)
- 跨越 session:本会话(主 agent 对 ee734f1/7e91add/b4f429d 三 commit 的事后逐行审查)
