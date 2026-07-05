# add-session-picker-and-exit-guard

## Why

两个 TUI 退出/恢复体验缺口(真机暴露),属 §13 路线 **1.4 TUI 体验**:

1. **空闲态单次 Ctrl+C 直接退出**(`should_exit` tui/mod.rs:655 最后一行,`Char('c')+CONTROL` 无条件 `true`),易误触即丢当前输入/会话;对标 Claude Code 应**连按两次**才退。
2. **`--resume` 只静默续最近会话**,无法选择 resume 哪个;对标 Claude Code `claude --resume` 应**列出历史会话供选**。上一个 change(add-session-persistence)已把会话落盘为 `sessions/<uuid>.jsonl`,列表数据已具备。

## What Changes

1. **Ctrl+C 分两态**(审查:现状 running / idle 都退出、中断只 Esc,不符预期):**agent 运行中 Ctrl+C → 中断当前轮**(对齐 Esc 现有中断 + Claude Code);**空闲态 Ctrl+C → 双击退出**——首次不退 + 活动行提示「再按一次 Ctrl+C 退出」+ 记时,`EXIT_DOUBLE_TAP`(1s)内再按 → 退出、超时重置。2 态纯函数 `exit_intent_action`(`{Consumed, Exit}`,排除项由调用方 gate)+ `last_exit_intent_at`(仿 `cancel_action` + `last_cancel_at`)。既有 selection(复制)/queue(清队)/permission/completion 时 Ctrl+C 处理不变;**运行中 Ctrl+C → 中断**(经扩 `app.rs:1109` 的 Esc 中断条件,追平基线 tui-shell「运行中可中断」spec——现状 code 未追平)。
2. **交互式会话列表(A1)**:`--resume` 进 TUI 后弹 `SessionPicker` modal(仿 `ModelsPicker`),列出历史会话(短 id / 时间 / 首条 `User` 摘要,mtime 逆序);选中 → **运行时 hot-swap**(idle 态 lock 替换 `agent_history` + `state.transcript` + 经 `SetProvider` channel 还原 provider/model + **同步 `state.session`**);`Esc` 取消 → 保持进入时的会话。键路由为 **early route**(picker 打开吃所有键、先于退出/滚动)。`SessionStore` 加 `list_sessions()`。
3. **`--continue` 续最近**:`--resume` 语义变「列出选」后,`--continue`(无参)复用现成 load-latest 续最近会话、不弹 picker;`--resume` / `--continue` 互斥、`--resume` 优先。

## Impact

- 修改 capability:`tui-shell`(Ctrl+C 两态守卫 + 会话选择 modal + 键 early route);`session-persistence`(`--resume` MODIFIED 为「列出选」+ `--continue` ADDED + `list_sessions` ADDED)
- Affected code:`src/tui/mod.rs`(`should_exit` 删裸 Ctrl+C + running 中断/idle exit-intent 接线 + picker early route + hot-swap + `StartupMode`)、`src/tui/app.rs`(`SessionPicker` + `pending_session_switch` + `last_exit_intent_at` + hot-swap 注入)、`src/tui/render.rs`(picker 渲染 + exit-intent 提示优先级)、`src/session/mod.rs`(`list_sessions`)、`src/main.rs`(`--resume` / `--continue` 解析 + `StartupMode`)
- **无新依赖**
- 回退:Ctrl+C 连按是纯增(单次不再退、连按等价原单次退出);`--resume` 无历史会话时 picker 为空 → 按当前(新)会话继续;非 `--resume` 冷启动行为完全不变
