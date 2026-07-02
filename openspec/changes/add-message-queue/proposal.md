## Why

agent 运行中提交的消息现在直接 `input_tx.send` 进 channel(`input_rx`)排队 + 立即 push 进 transcript,且无条件 `reset_turn_token_usage()` + `iteration=0`(app.rs:1033-1037)——**channel 层排队不可见/不可取消/不可编辑,还会污染当前正在跑那轮的 token/迭代显示**。用户要 Claude Code 式排队(参考截图):running 时提交的消息进一个**可见队列**(显示在输入框上方、spinner 下方,每条 `⟩` 前缀),当前轮完成后才 pop 一条进 transcript 处理;`Esc`/`Ctrl+C` 两级取消(第 1 次中断当前轮 + 立即发下一条、紧接再按清空所有排队)。

## What Changes

- **`AppState` 加 `pending_queue: Vec<String>` + `queue_cancel_armed: bool`**。
- **提交分流**:`phase == Ready` **且 `!has_queue()`** → 现状直发(push transcript + send + Busy + reset 本轮);**否则**(running,或 Ready 但队列非空)→ 入队 `pending_queue`,**不 send、不 push transcript、不 reset、不动 iteration**(当前轮零污染;Ready-有队列也入队以保 FIFO,防 Ready 窗口内提交插队)。命令(`/xxx`)running 时仍即时执行、不入队。
- **turn 完成推进**:事件循环 ui_rx 分支处理完 **`TurnComplete` / `Interrupted` / `Error`**(三个真终止事件之一;非 `StatusChanged(Idle)`)后,若 `has_queue()` → `dequeue_next()`(pop + push transcript `User` + `phase=Busy` + `reset_turn_token_usage()`)并 `send(Prompt)`。`Error` 亦推进(否则报错路径队列卡死)。**删中断臂冗余 `StatusChanged(Idle)` send** + **`apply(Idle)` 不再置 `phase=Ready`**(phase→Ready 统一由三终止事件驱动):消除闪帧、`new_message_count` 误增,及"正常 `Idle→TurnComplete` 间露 Ready 窗口致陈旧 TurnComplete 误推进+错序"(第二轮 finding 3)。channel 恒最多一条。
- **取消两级(时间窗)**:有排队时——第 1 次 `Interrupt` 中断 + 记 `last_cancel_at`(推进随即发下一条)、**快速连按**(两次取消键 `gap < 600ms`)`clear_queue` 清空所有。**用到达时间间隔判定、不用布尔 armed**:第 1 次取消必触发推进(往返亚毫秒),布尔 armed 会被推进清掉致"清空档"结构性不可达(第二轮 findings 1/2/4);时间窗则推进不碰 `last_cancel_at`——快速连按可达清空、隔久单按判第 1 次不误清(顺带修 finding 9 跨轮粘滞)。
- **渲染**:`QUEUE_MAX_ROWS=5` 封顶;布局在活动行与输入框间插排队区(`queue_height=min(len,5)`),render() rows 索引相应后移;**`input_content_height_cap` 减去 `queue_height`**(防排队区从 transcript 地板偷行 / 输入框被压)。
- **v1 不做 ↑编辑排队**(↑ 维持输入历史/多行光标)。

## Capabilities

### New Capabilities

- `tui-shell`:
  - **ADDED**:`消息排队(running 时提交入 app 层可见队列)` —— 入队、输入框上方渲染(QUEUE_MAX_ROWS 封顶 + cap 核算)、三终止事件推进 pop、Esc/Ctrl+C 两级取消。

### Modified Capabilities

- `tui-shell`:
  - **MODIFIED**:`运行中可中断(Esc 中断本轮)` —— 中断路径只发 `Interrupted`(删冗余 Idle) + `apply(Idle)` 不置 Ready(phase→Ready 仅三终止事件);Esc/Ctrl+C 分流加"取消排队"两级(**时间窗:快速连按清空**);排队移到 app 层 `pending_queue`,channel 恒最多一条。

## Impact

- **代码**:
  - `src/tui/app.rs`:`pending_queue`/`last_cancel_at`;Enter 提交分流(Ready+!has_queue 直发,否则入队;不污染当前轮);`enqueue_prompt`/`dequeue_next`(含 reset token)/`clear_queue`/`has_queue`;`apply(Idle)` 不置 Ready + bump 移终止事件。
  - `src/tui/mod.rs`:ui_rx 分支三终止事件后 `dequeue_next`+`send`;**删中断臂 `StatusChanged(Idle)` send**(同步更新中断/状态行测试);`should_exit`/Esc/`Ctrl+C` 分流加两级取消**时间窗**(`cancel_action` + `CANCEL_DOUBLE_TAP`,推进不碰 `last_cancel_at`)。
  - `src/tui/render.rs`:`QUEUE_MAX_ROWS`;`layout_rows` 插排队区 + rows 索引后移;`render_queue`。
  - `src/tui/input_layout.rs`:`input_content_height_cap` 减 `queue_height`。
- **依赖**:零新增。
- **测试**:队列动作/提交分流/取消两级+armed 清除 —— 纯逻辑单测;推进(含 Error 收场、中断无闪帧)集成验证;排队区渲染 insta 快照 + 高度回归单测。
- **风险**:见 design(Error 推进语义、armed 集中清除、删 Idle 影响既有中断测试、layout 地板核算);propose 已过第一轮对抗审查(11 CONFIRMED 已修入),将再过一轮验证。
