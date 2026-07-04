# Tasks — auto-context-window-and-copy-hint

## 1. 内核:窗口解析与默认启用(强制 TDD)

- [x] 1.1 RED:`provider/model_meta.rs` 测试(解析优先级 / 大小写 / 特定遮蔽 / o 系边界 / 未知默认 65_536),运行确认失败原因正确(6 failed,`not yet implemented`)
- [x] 1.2 GREEN:内置表 + `context_window_for` + `resolve_context_window` 最小实现(6/6 过)
- [x] 1.3 RED:`Compacting` 判定时解析(settings `Option` 覆盖;`set_model` 后同 usage 翻转触发)——断言级红灯:切 gpt-4 后未触发压缩(适配层未接表)
- [x] 1.4 GREEN:`CompactionSettings.model_context_window: Option<u32>`、`exceeds_threshold` 实时解析、`run_compact_command(&Compacting, ..)`;`assemble_agent` 始终注入 Compacting、`AssembledAgent.compacting` 去 Option;适配 tui/mod.rs 接线与既有测试(含装配 Passthrough 断言改写)

## 2. TUI:复制 hint(事后测试)

- [x] 2.1 `AppState.copy_hint` + `set_copy_hint` / `active_copy_hint(now)`(TTL 4s)+ 单测(存续 / 过期 / 覆盖)
- [x] 2.2 `copy_selection` 成功路径改 set hint(失败 Notice 不动);更新 clipboard.rs 两个既有测试(断 hint + transcript 不增)
- [x] 2.3 `render_activity` 右对齐渲染 hint(宽度不足让位)+ 带色快照(新增 `tui_activity_copy_hint`);既有快照零漂移(git status 仅新增 1 个 .snap)
- [x] 2.4 真机核验:复制后输入框右上出现 hint、约 4s 消失、transcript 无「已复制」Notice(用户确认)

## 3. 真机反馈修订:压缩进行态

- [x] 3.1 内核红绿:成功 notice 去计数(先改断言确认红:左值「已压缩上下文:8 → 3 条消息」,再改实现为「已压缩上下文」)
- [x] 3.2 `Phase::Compacting` + `AgentEvent::CompactDone`;`/compact` 发起门控(Ready 且无排队,否则 notice 拒绝);agent task 收场双发 `Notice` + `CompactDone`(成功 / 失败均收场)
- [x] 3.3 推进闸门扩为四事件(TurnComplete / Interrupted / Error / CompactDone);集成测试 `compact_done_advances_exactly_one_queued_prompt`(推进恰一条、channel 恒最多一条)
- [x] 3.4 activity line 压缩动画(spinner「压缩上下文…」,accent、无 esc 提示)+ 快照 `tui_activity_compacting`;app 层门控 / 收场单测 3 个
- [x] 3.5 真机:/compact 出现动画;期间提交进可见队列、压缩完自动推进;notice 无计数(用户确认)

## 4. 门禁

- [x] 4.1 `cargo test --lib` 全绿(503 passed / 0 failed);`cargo clippy --all-targets -- -D warnings` 零警告
- [x] 4.2 `openspec validate auto-context-window-and-copy-hint --strict` 通过
- [x] 4.3 真机:未配 `model_context_window` 时 `/compact` 可压、全程无「压缩未启用」提示(用户确认)
