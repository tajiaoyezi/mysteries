## 1. 队列状态 + 动作(纯逻辑 · RED 停点)

- [x] 1.1 `src/tui/app.rs`:`AppState` 加 `pending_queue: Vec<String>`(初始空)、`last_cancel_at: Option<Instant>`(初始 None)。方法:`enqueue_prompt`;`dequeue_next(&mut self) -> Option<String>`(pop 队首;有值时 push `TranscriptBlock::User(消息)` + `phase=Busy` + `reset_turn_token_usage()`,返回消息);`clear_queue`;`has_queue`。**先只写单测跑红**(新接口,贴红停等确认):enqueue 追加队尾;dequeue_next 空→None、非空→Some(队首) 且 transcript +1 `User`、phase=Busy、turn token 归零、余项保序;clear_queue 后空 + has_queue=false。再实现转绿。

## 2. 提交分流(running/Ready-有队列 入队,不污染当前轮)

- [x] 2.1 `on_key_inner` Enter 分支:`prompt` 非空、非命令后——`phase == Ready` **且 `!has_queue()`** → 现状直发;**否则** → `enqueue_prompt`,不 send/不 push/不 reset/不动 iteration。命令 running 时仍即时 `execute_command`、不入队。
- [x] 2.2 单测:running 态(iteration=N、turn token 非零)提交 → 入队、iteration/turn token **未变**;`Ready` 但队列非空提交 → 入队(保 FIFO);`Ready` 且空队列 → 直发(回归)。

## 3. 推进 + Idle 不置 Ready + 删中断臂 Idle(事件循环接线)

- [x] 3.1 `src/tui/mod.rs` ui_rx 分支:`apply_ui_event` 后,event ∈ `{TurnComplete, Interrupted, Error}` 且 `has_queue()` → `dequeue_next()` 的 `Some(p)` 执行 `send(Prompt(p))`(同一唤醒内);`StatusChanged(Idle)` **不**推进。
- [x] 3.2 `src/tui/app.rs` `apply(StatusChanged(Idle))` **不再置 `phase=Ready`**(phase→Ready 仅由 `TurnComplete`/`Interrupted`/`Error` 分支);`was_busy && Ready → bump_new_message_count`(现 app.rs:869)**移到三终止事件分支**。核查既有对 Idle→Ready 的依赖(状态行渲染、测试断言)并同步更新。
- [x] 3.3 **删** `run_agent_task` 中断臂 mod.rs:787 的 `StatusChanged(Idle)` send(仅留 `Interrupted`);同步更新既有中断测试(如 `run_agent_task_interrupts_running_prompt`)对 Idle 的断言。
- [x] 3.4 集成验证(run_agent_task 测试风格,无终端):① A `TurnComplete` + 排队 B → B 推进跑完,channel 全程最多一条;② A `Error` + 排队 B → B 仍推进;③ 中断 A + 排队 B → B 推进,无尾随 Idle;④ Idle→TurnComplete 间提交 X → X 入队(不直发)、无陈旧 TurnComplete 误推进。

## 4. 取消两级时间窗(纯逻辑 · RED 停点)

- [x] 4.1 `cancel_action(gap: Duration, threshold: Duration) -> CancelAction`(`gap >= threshold` → `InterruptAndAdvance`,否则 `ClearAll`)。**先写单测跑红**(新接口,贴红停等确认):`gap >= threshold`→`InterruptAndAdvance`;`gap < threshold`→`ClearAll`;边界 `gap == threshold`→`InterruptAndAdvance`。再实现转绿。
- [x] 4.2 接线 `src/tui/mod.rs` `should_exit`/Esc/`Ctrl+C` 分流 + `app.rs`:`const CANCEL_DOUBLE_TAP: Duration = Duration::from_millis(600)`;`has_queue()` 时取消键计 `gap = now.duration_since(last_cancel_at.unwrap_or(远古))`,按 `cancel_action`——`InterruptAndAdvance`→`Interrupt` + `last_cancel_at=Some(now)`、`ClearAll`→`clear_queue()` + `Interrupt`。**推进/其它路径不触碰 `last_cancel_at`**。优先级 pending>选区>有排队两级取消>运行中中断(无排队)>就绪退出。

## 5. 渲染排队区 + 高度核算(TUI 外壳 · 事后快照 + 高度单测)

- [x] 5.1 `src/tui/render.rs`:`const QUEUE_MAX_ROWS: usize = 5;`;`queue_height = min(len, QUEUE_MAX_ROWS)`;`layout_rows` 活动行(rows[4])与输入框间插 `Constraint::Length(queue_height as u16)`,**render() 输入框/状态/MODE rows 索引 +1**;`render_queue` 逐条 `⟩ ` + 首行(多行 + `…`,超上限末行 `⟩ …(+N)`)。
- [x] 5.2 `src/tui/input_layout.rs`:`input_content_height_cap` **减 `queue_height`**;高度单测:queue>0 且 input 顶满时 input 内容行数 = 预期(不偷 transcript 地板、input 不压到 0);最小屏高(24、gap=2、queue=5)transcript ≥ 8。
- [x] 5.3 insta 快照:两条排队(含多行);空队列布局同现状。

## 6. 校验

- [x] 6.1 `cargo test --lib` 全绿 + `cargo clippy --all-targets -- -D warnings` 零警告 + `openspec validate add-message-queue --strict` 通过;**真机复核**:running 提交多条 → 排队区显示;当前轮(完成/中断/出错)后依次处理;第 1 次 Esc/Ctrl+C 中断+发下一条、**快速连按**清空所有、隔久再按仍按第 1 次;命令 running 即时执行不入队;当前轮 token/迭代不污染;中断/推进无 Ready 闪帧;Idle→TurnComplete 间提交不错序;排队多条不挤穿 transcript。
