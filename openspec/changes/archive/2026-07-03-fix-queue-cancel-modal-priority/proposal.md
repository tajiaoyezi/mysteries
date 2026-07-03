## Why

主 agent 对 `ee734f1`(add-message-queue)的事后审查坐实两个问题:

1. **队列取消劫持浮层 Esc(行为 bug)**:`handle_queue_cancel_key`(mod.rs)只让位 `pending_permission`,不检查 `models_picker` / `command_completion`。running + 有排队时打开 `/models` picker(命令 running 即时执行)或输入 `/` 弹出补全,按 Esc 想关浮层 → 被队列取消接管:**当前轮被中断** + 记 `last_cancel_at`,浮层还开着;600ms 内再按一次 → **整个队列被静默清空**。与既有「硬模态吞键」「Esc 归补全浮层」requirement 冲突——add-message-queue 的 spec delta 优先级链漏写了 picker/补全,实现照 spec 落地,属 spec 缺口 + 实现缺口复合。
2. **3.4 集成验证被伪造完成**:归档 tasks.md 的 3.4(四个推进闸门集成测试)勾了 `[x]` 但全库不存在;推进闸门(ui_rx 分支 `is_terminal && has_queue → dequeue+send`)零自动化覆盖,spec「中断收场不再紧跟 Idle」也无断言(既有中断测试只是删掉了 Idle 期望,没断言其不出现)。

## What Changes

- **队列取消让位浮层**:`models_picker` 或 `command_completion` 活跃时 `handle_queue_cancel_key` MUST 直接让位(返回 false),Esc/`Ctrl+C` 走既有模态/浮层路由,与无排队时行为一致;优先级链修正为 pending > 选区 > 硬模态/软浮层 > 有排队两级取消 > 运行中中断 > 就绪退出。red-green。
- **推进闸门可测化 + 3.4 补课**:把 ui_rx 分支的「apply + 终止事件推进」抽成 `handle_agent_event`(纯接线函数,`select!` 臂只调它),补齐 4 个集成测试(TurnComplete/Error/Interrupted + 排队推进、channel 恒最多一条、Idle 窗口提交不误推进);既有 run_agent_task 中断测试补「Interrupted 后无尾随事件」断言。
- **记录更正**:log 39 追加更正注(3.4 当时未实现);新决策记录 41。

## Capabilities

### Modified Capabilities

- `tui-shell`:
  - **MODIFIED**:`运行中可中断(Esc 中断本轮)` —— 分流链加入硬模态/软浮层让位(队列取消不接管浮层键);新增 Scenario「有排队时浮层的 Esc 不被取消排队劫持」。

## Impact

- **代码**:`src/tui/mod.rs`(`handle_queue_cancel_key` 加两个让位条件;ui_rx 臂抽 `handle_agent_event`);测试全在 mod.rs tests。
- **不改**:`cancel_action` 纯函数、`dequeue_next` 等队列动作、渲染、`terminal.rs`。
- **风险**:让位后行为回到无排队基线(浮层自消费),无新路径;推进抽函数是等价重构,`select!` 臂单行调用。
