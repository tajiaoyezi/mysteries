# 2026-07-04 · 50 · archive-add-session-picker-and-exit-guard

## 决策
- Ctrl+C 拆两态:running 中断 / idle 双击退出 | 选:running Ctrl+C 追平 Esc 中断(扩 app.rs 现有中断条件为 `(Esc||Ctrl+C)&&is_running()`) | 弃:running Ctrl+C 退出(原状、破坏性)、在 process_event_batch 单独接线 running(H-2:抢占 selection@copy/queue-cancel 次序,is_running 与 has_queue 可并存) | 主导:用户拍板(定案1) | 依据:baseline tui-shell spec 302 早已要求运行中中断而 code 未追平 + Claude Code 对标
- idle 退出用双击守卫 | 选:`exit_intent_action(gap,threshold)->{Consumed,Exit}` 2 态、排除项由调用方 gate、`last_exit_intent_at` 记时、`EXIT_DOUBLE_TAP=1s` | 弃:3 态含 NotHandled(M-1:签名只有 gap/threshold 产不出) | 主导:审查收敛 | 依据:对齐 `cancel_action` 范式
- hot-swap 续写选中会话且补 `state.session` | 选:idle lock 替换 agent_history/transcript + SetProvider 还原 provider + 同步 `state.session.provider/model` + `session_meta`(let mut) | 弃:漏 `state.session`(H2:`write_session_snapshot`/footer 读它 → 续写污染 + footer 错) | 主导:审查收敛 + 用户拍板(定案2) | 依据:code
- 键路由 = early route | 选:picker 打开时 `press_index+=1` 后集中吃所有键、先于退出/滚动/selection/queue | 弃:分散镜像 ModelsPicker 多处守卫(审查逐条点出 Esc 误退 / 箭头滚 transcript / 字符漏输入) | 主导:审查收敛 + 用户拍板(定案4) | 依据:code(should_exit 无 session 守卫)
- --resume 语义改「列出选」+ 新增 --continue 续最近 | 选:--resume 弹 SessionPicker(仅 1 会话仍弹)、--continue 复用 load-latest、二者 resume 优先 | 弃:--resume 静默续最近(原状、无法选) | 主导:用户拍板(定案3、A1) | 依据:spec
- 活动行提示优先级 exit-intent > copy_hint > paste | 主导:审查(M-4:上膛警告被 copy_hint 遮则守卫失效) | 依据:code + insta 护栏

## 变更
- code:`src/tui/mod.rs`(StartupMode/startup_mode、ExitIntent/exit_intent_action、handle_idle_exit_intent_key、should_exit 裸 Ctrl+C→false、picker early route、hot-swap 六步、run_tui resume→弹 picker / continue→load-latest);`src/tui/app.rs`(SessionPicker/SessionRow、handle_session_picker_key catch-all、running Ctrl+C 扩中断条件、last_exit_intent_at/pending_session_switch);`src/tui/render.rs`(render_session_picker、activity 优先级);`src/main.rs`(--resume/--continue 解析剥离 + startup_mode);`src/session/mod.rs`(list_sessions/SessionSummary、Reverse 排序)
- spec:tui-shell(MODIFIED 基线「运行中可中断」分流链 + ADDED 会话选择 modal);session-persistence(MODIFIED --resume 列出选 + ADDED --continue / list_sessions)
- 快照:新增 tui_session_picker_open、tui_activity_exit_intent_priority

## 待决 / 已知限制
- hot-swap 到 config 已删 provider 的会话:agent 回退默认 + Notice(spec 硬要求 no-panic/fallback/Notice 满足),但 `state.session`/footer 仍显旧 provider 名(cosmetic 分歧,未修)
- SessionPicker 宽度 min(68),宽终端右缘露底(与 ModelsPicker 一致,未修)
- 执行 agent 第 3 次全仓 `cargo fmt`:9 个零内容文件已 `git checkout --` 还原,5 个 feature 文件保留 contained fmt(用户接受)

## 引用
- OpenSpec change:add-session-picker-and-exit-guard(tui-shell / session-persistence deltas)
- 跨越 session log:本 session(3.1 停点审查 + 4.x 两轮对抗审查 + 收尾亲验);复用 add-session-persistence(log 49)的 replace_system_head(D8)/ SetProvider(D9)
