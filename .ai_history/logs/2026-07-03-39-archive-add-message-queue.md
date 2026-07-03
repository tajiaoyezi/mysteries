# 2026-07-03 · 39 · archive add-message-queue

## 决策
- **排队从 channel 层移到 app 层可见 `pending_queue`** | 选:app 层 `Vec<String>`(输入框上方 `⟩` 前缀渲染、可取消) | 弃:保留 channel 层排队仅给 transcript User 块加标识(不可见/不可取消/不可管理,不符 Claude 截图) | 主导:用户(照 Claude Code 截图定样式) | 依据:spec
- **提交分流:`Ready && !has_queue` 直发,否则入队** | 当前轮零污染(不 send/不 push/不 reset token/不动 iteration);`Ready` 但队列非空也入队以保 FIFO(防 Ready 窗口内提交插队) | 主导:讨论收敛 | 依据:code(app.rs Enter 分支)
- **推进闸门 = 三终止事件枚举 `{TurnComplete, Interrupted, Error}`** | 选:枚举三事件(各路径恰推进一次) | 弃:「phase 落 Ready 即推进」——正常路径 `Idle`+`TurnComplete` 双触发致重复推进两条;弃:推进放 `app.apply`——无 `input_tx` 且被测试大量直调 | 主导:第一轮对抗审查 | 依据:code/tests。`Error` 必须在内,否则 provider 报错路径队列卡死
- **`apply(StatusChanged(Idle))` 不再置 `phase=Ready` + 删中断臂冗余 `StatusChanged(Idle)` send** | phase→Ready 统一由三终止事件驱动 | 消除:Busy→Ready→Busy 闪帧、`new_message_count` 误增、正常 `Idle→TurnComplete` 间的 Ready 直发窗口(陈旧 `TurnComplete` 撞直发新轮致误推进+错序,第二轮 finding 3);`bump_new_message_count` 相应移到三终止事件分支 | 主导:第二轮对抗审查 | 依据:code
- **两级取消用时间窗 `last_cancel_at` + `cancel_action(gap, threshold)`,弃布尔 `armed`** | 选:`gap >= 600ms`→`InterruptAndAdvance`(中断当前+推进下一条)、`gap < 600ms` 快速连按→`ClearAll`(清空所有);**推进不触碰 `last_cancel_at`** | 弃:布尔 `queue_cancel_armed`——第 1 次取消必触发推进(往返亚毫秒),推进清 armed 则人手第 2 次按键(~150ms)前 armed 已被清、「清空档」结构性不可达(第二轮 findings 1/2/4);推进不清 armed 又跨轮粘滞误清(finding 9) | 主导:用户(选 A 时间窗) | 依据:tests(`cancel_action` 纯函数单测)
- **渲染 `QUEUE_MAX_ROWS=5` + `input_content_height_cap` 减 `queue_height`** | 排队区插活动行与输入框间;空队列零高度、布局同现状(既有 31 快照零 churn);render() rows 索引由 `input_row`(无队列=5/有队列=6)派生 status/mode,无遗漏 | 主导:讨论收敛 | 依据:code/快照

## 变更
- `src/tui/app.rs`:`pending_queue`/`last_cancel_at`(+getter/setter);`enqueue_prompt`/`dequeue_next`(pop 队首 + push User + Busy + `reset_turn_token_usage`)/`clear_queue`/`has_queue`;Enter 提交分流;`apply(Idle)` 不置 Ready + bump 移三终止事件。
- `src/tui/mod.rs`:`CANCEL_DOUBLE_TAP=600ms` + `enum CancelAction` + `cancel_action`;`handle_queue_cancel_key`(优先级 pending>选区>有排队两级取消>运行中中断>就绪退出);ui_rx 三终止事件后 `dequeue_next`+`send`;删中断臂 `StatusChanged(Idle)` send。
- `src/tui/render.rs`:`QUEUE_MAX_ROWS`;`queue_height`;`layout_rows` 条件插排队区;`render_queue`(`⟩` 前缀 + 首行 `…` + 超限 `⟩ …(+N)`)。
- `src/tui/input_layout.rs`:`input_content_height_cap` 增 `queue_height` 入参并减去。
- 新快照 `tui_queue_area.snap`;既有快照零 churn。`cargo test --lib` 450 passed / 0 failed;clippy 零警告;真机复核通过。

## 待决
- **↑ 编辑排队消息**:v1 未做(↑ 维持输入历史/多行光标),按需再议。
- **折叠占位 `[Pasted text #N +M lines]`**:下一 change(需 brainstorm→propose)。
- `CANCEL_DOUBLE_TAP=600ms` 阈值真机手感可再调。

## 引用
- OpenSpec change:`add-message-queue`(propose 90c8b2e,三轮对抗审查 11+4+0 CONFIRMED 收敛)。
- 相关 log:[[2026-07-01-37-archive-guard-paste-burst-submit]]、[[2026-07-02-38-archive-guard-paste-cross-batch]](同属 TUI 粘贴/交互健壮化系列,crossterm Windows 限制背景一脉)。
