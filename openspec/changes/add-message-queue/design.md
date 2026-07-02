## Context

现状:agent 运行中(`phase.is_running()`)按 Enter,`on_key_inner`(app.rs:1018-1038)无 running 守卫,直接 `phase=Busy` + push transcript `User` + `input_tx.send(Prompt)` + `reset_turn_token_usage()` + `iteration=0`。排队靠 channel(`input_rx` unbounded),`run_agent_task`(mod.rs:739)`while let recv` 逐个处理;`Prompt` 分支以 `tokio::select!` 把本轮与独立中断信号并置——`Ok` 发 `TurnComplete`、`Err` 发 `Error`(mod.rs:780)、中断发 `Interrupted` **且紧跟一条 `StatusChanged(Idle)`**(mod.rs:786-787)。问题:channel 排队**不可见/不可取消/不可编辑**;且 reset 污染当前正在跑那轮的 token/迭代。

事件循环(mod.rs:130-183):`select!`{events / ui_rx / spinner};ui_rx 分支(157-169)调 `apply_ui_event(&mut state, event, ...)`(无 `input_tx`),该分支作用域**有** `input_tx`;每处理一条 ui 事件即 `draw` 一帧(ui 事件无 batching)。布局(render.rs:56-67):rows[4]=活动行、rows[5]=输入框、rows[6]=状态行、rows[7]=MODE;`input_content_height_cap`(input_layout.rs) = `screen_height - 16 - gap - permission`(固定和 16 未含排队区)。参考 Claude Code(用户截图):排队消息在 spinner(活动行)下、输入框上,每条 `⟩` 前缀。

## Goals / Non-Goals

**Goals:**
- 排队从 channel 层移到 **app 层可见队列** `pending_queue`,输入框上方渲染。
- running 提交入队、**不污染当前轮**;当前轮以**任一终止态**(完成/中断/出错)收场后 pop 一条进 transcript 处理,保 FIFO。
- Esc/Ctrl+C 两级取消:第 1 次中断当前 + 推进下一条、紧接再按清空所有排队。

**Non-Goals:**
- v1 不做 ↑ 编辑排队消息(↑ 维持输入历史/多行光标)。
- 不改 `run_agent_task` 逐条处理 / 独立中断通道的既有机制。
- 不做队列持久化 / 跨会话。

## Decisions

- **D1 `AppState.pending_queue: Vec<String>` + `last_cancel_at: Option<Instant>` + 队列动作(纯逻辑可测)。** `enqueue_prompt(s)` push 队尾;`dequeue_next() -> Option<String>` pop 队首**并**(有值时)push transcript `User` + `phase=Busy` + **`reset_turn_token_usage()`**(推进新轮清零上一轮 token,修跨轮污染),返回该消息供调用方 send;`clear_queue()`;`has_queue()`。`last_cancel_at` 记最近一次"第 1 次取消键"的时刻,供 D4 时间窗判定;**推进不触碰它**。队列动作纯 app 状态、可单测。

- **D2 提交分流(on_key_inner Enter)。** `prompt` 非空、非命令后:**`phase == Ready` 且 `!has_queue()`** → 直发(push transcript + send + Busy + reset 本轮);**否则(running,或 Ready 但队列非空)** → `enqueue_prompt(prompt)`,**不 send、不 push transcript、不 reset、不动 iteration**。`Ready 但队列非空` 也入队是为防"正常完成的 Ready 窗口内提交直发插队、打乱 FIFO"(见 D3)。运行中的**命令**(`/xxx`,单行 `parse_command` 命中)仍即时 `execute_command`、不入队。

- **D3 推进触发 = 三个真终止事件 `{TurnComplete, Interrupted, Error}`(非 `StatusChanged(Idle)`)。** 放事件循环 ui_rx 分支(需 `input_tx`):`apply_ui_event` 后,若刚处理的 event ∈ `{TurnComplete, Interrupted, Error}` 且 `has_queue()`,调 `dequeue_next()` 并对返回消息 `send(Prompt)`。**为何不用"phase 落 Ready 即推进"**:正常完成路径 `run_agent_task` 先发 `StatusChanged(Idle)`(phase→Ready)再发 `TurnComplete`,两者都令 phase→Ready,若以 phase 电平为闸门会**重复推进两条**;枚举三终止事件则各路径恰推进一次。**`Error` 必须在内**(provider 403/限流/max_iterations 走 `Error` 收场),否则队列在报错路径卡死。**删中断臂 mod.rs:787 冗余的 `StatusChanged(Idle)` send**:`Interrupted` 的 apply 已置 `phase=Ready`,该 Idle 冗余;不删则推进置 Busy 后被尾随 Idle 拉回 Ready → 可见 Busy→Ready→Busy 闪帧 + `new_message_count` 误 +1(需同步更新既有中断测试对 Idle 的断言)。**消除正常路径 `Idle→TurnComplete` 的 Ready 直发窗口(修 double-send,第二轮 finding 3)**:`apply(StatusChanged(Idle))` **不再置 `phase=Ready`**——turn 结束统一由三终止事件置 Ready(与删中断臂 Idle 一致);故 `Idle` 到达后 phase 仍运行态、用户此刻提交走**入队**而非直发,杜绝"陈旧 `TurnComplete` 撞上直发新轮 → 误推进 + 错序"。原 `was_busy && Ready → bump_new_message_count`(app.rs:869)相应移到终止事件分支。**channel 恒最多一条**由此成立:phase→Ready 仅由终止事件驱动(无 Idle 制造的中间 Ready 窗口),推进(终止事件后 has_queue)与 idle 直发(`!has_queue`)互斥。

- **D4 取消两级,用"两次取消键的到达时间间隔"判定(时间窗,**不用布尔 armed**)。** 事件循环/AppState 持 `last_cancel_at: Option<Instant>`。仅 `has_queue()` 时 Esc/`Ctrl+C` 接管取消(否则维持 pending>选区>运行中中断>就绪退出):设 `gap = now.duration_since(last_cancel_at)`(首次为 ∞)——`gap >= CANCEL_DOUBLE_TAP`(默认 600ms;第 1 次/非连按)→ `send(Interrupt)` 中断当前 + 记 `last_cancel_at = now`(随后 `Interrupted` 触发 D3 推进下一条,即"中断当前+发下一个");`gap < CANCEL_DOUBLE_TAP`(**快速连按**)→ `clear_queue()` + `send(Interrupt)`("清空所有排队")。判定抽纯函数 `cancel_action(gap, threshold) -> {InterruptAndAdvance, ClearAll}`(`gap>=threshold`→前者),`Instant` 只在事件循环算 `gap` 传入、不入纯逻辑。**为何弃布尔 armed(第二轮 findings 1/2/4 坐实)**:第 1 次取消**必然**触发推进(`Interrupted` 往返亚毫秒),若"推进清 armed"则第 2 次人手按键(~150ms)时 armed 已被清、清空档结构性不可达;而"推进不清 armed"又会跨整轮粘滞(隔久单按误清空,finding 9)。时间窗两难全解:推进不碰 `last_cancel_at`,快速连按 `gap<阈值` → 清空可达;隔久单按 `gap` 大 → 判第 1 次(不误清)。优先级:pending > 选区 > 有排队两级取消 > 运行中中断(无排队) > 就绪退出。

- **D5 渲染排队区 + 高度核算。** `QUEUE_MAX_ROWS = 5`(具名常量);`queue_height = min(pending_queue.len(), QUEUE_MAX_ROWS)`(0 则不占行)。`layout_rows` 在 rows[4](活动行)与输入框间插 `Constraint::Length(queue_height)`(**render() 里输入框/状态/MODE 的 rows 索引相应后移**)。`render_queue` 逐条渲一行 `⟩ ` + 消息首行(多行取首行 + `…`),超 `QUEUE_MAX_ROWS` 时末行 `⟩ …(+N)`。**`input_content_height_cap` 公式须减去 `queue_height`**(否则排队区从 transcript 地板偷行 / input 被过约束压缩);且保证最小可用屏高(24 行)下 `header3 + 地板8 + gap + activity1 + QUEUE_MAX_ROWS + input_min + status1 + mode1 ≤ screen`(QUEUE_MAX_ROWS=5 满足)。空队列零高度、布局同现状。

## Alternatives considered

- **推进以"phase 落 Ready"为闸门**(审查建议之一)——正常路径 `Idle`+`TurnComplete` 双触发 → 重复推进两条。改用枚举三终止事件。弃。
- **保留 channel 层排队 + 仅给 transcript User 块加标识**(早期设想)——不可取消/管理、不符 Claude 截图。弃。
- **推进放 app.apply**——app.apply 无 `input_tx` 且被测试大量直接调用。放 ui_rx 分支。弃。
- **两级取消用布尔 `queue_cancel_armed`**(第二轮前)——第 1 次取消必触发推进、推进(若清 armed)在人手第 2 次按键前清掉 armed → 清空档结构性不可达(findings 1/2/4);"推进不清 armed"又跨整轮粘滞误清(finding 9)。改时间窗两难全解。弃。

## Risks / Trade-offs

- **`Error` 收场推进语义**:选择"继续 pop 下一条"(避免卡死 + Esc 陷阱);若首条因 provider 配置错持续 `Error`,后续排队会接连报错——用户可 Esc 两级清空。比"卡死"优。
- **取消时间窗手感**:`CANCEL_DOUBLE_TAP=600ms` 是双击窗口,"清空所有"要求两次取消键在此窗口内连按;隔久再按被判"第 1 次"(中断+推进)。阈值真机调。
- **`apply(Idle)` 不再置 Ready 的连锁**:须核查既有对"Idle→Ready"的依赖(状态行渲染、既有测试断言、`bump_new_message_count` 时机),把 Ready 化统一到三终止事件;正常 turn 仅在**结束**出现 `Idle`(中途是 CallingModel/ExecutingTool),故影响面限于 turn 收尾一瞬,实现时须回归既有状态行/中断测试。
- **删中断臂 Idle send**:影响既有中断测试(断言 Idle 事件)——须同步更新。
- **多行排队消息渲染**:每条只显首行 + `…`,完整内容 pop 进 transcript 时可见。
- **命令在 running 时不入队**:`/xxx` 即时执行(如 `/clear`);与 Prompt 入队语义不同,spec 明确。
