# 2026-06-29 · 33 · archive add-jump-to-bottom-indicator

## 决策

- **跳到底部 pill + 新消息计数** | 主导:用户(实测「滚上去看不出是否在最新位置」+ 给 Claude Code 截图,要 jump-to-bottom + new messages)| 独立 change(滚动可视性),排在 ↑↓ 修复之后
- **D1 pill 仅未跟随底部时渲染,钉视口底行、局部 Clear(仅 pill 宽)** | 跟随底部完全不渲染;局部 Clear 避全宽黑带(沿用 models picker 教训)
- **D2 计数 = 已完成助手消息,per-turn(一轮答复 = 1)** | 主导:用户(AskUserQuestion 选「仅助手消息,一轮=1」,例「1 段+3 工具卡→1」)。**初版实现为 per-round**(每离开 `CallingModel` +1)→ 多轮 agentic 一轮提问会 +2,违反用户选择 + spec scenario;**且单测只走第一个过渡断言 ==1,半路假绿放过**。修:`was_busy = phase != Ready`(更新前取)+ `was_busy && phase == Ready`(转 Idle/Ready 才 +1);测试改完整一轮 `CallingModel→ExecutingTool→CallingModel→Idle` 断言 ==1
- **D3 文案中文 + 计数仅助手** | 主导:用户拍板。`跳到底部 (ctrl+End) ↓` / `N 条新消息 (ctrl+End) ↓`;工具卡 / user 回显 / notice 不计
- **D4 Ctrl+End 复用既有 `KeyCode::End` 路由** | `scroll_action_for_key` 按 code 匹配,`End`/`Ctrl+End` 同走 `scroll_to_bottom`;回底由 `follows_bottom` 转真清零,与按键无关
- **D5 复用既有 `follows_bottom: bool` 持久标志** | 实现期核实 `follows_bottom` 已是滚动动作维护的字段(`page_up`/`scroll_up` 置 false、`scroll_to_bottom` 置 true),无需补;计数在事件处理期判断不依赖渲染期 dims
- **审查**:独立 cargo/clippy + 读码 —— 查实 pill 局部 Clear 无黑带、ctrl+End 测试覆盖两键 + 清零、follows_bottom 字段/方法一致;**揪出 per-round 假绿**(单测停半路),要求 per-turn 修复

## 变更

- `src/tui/jump_to_bottom.rs`(新):`bump_new_message_count` / `new_message_count_on_follow_bottom` / `jump_to_bottom_pill_text` + 纯函数单测
- `src/tui/app.rs`:`AppState.new_message_count`;`StatusChanged` 处 per-turn bump(busy→Ready)+ 回底清零;助手计数 / 跟随不计 / 工具卡不计 测试
- `src/tui/mod.rs`:`end_and_ctrl_end_map_to_scroll_to_bottom_and_clear_new_message_count` 测试
- `src/tui/render.rs`:`render_jump_to_bottom_pill`(未跟随时视口底行局部 Clear + 两态文案 + ↓ glyph)
- spec:`tui-shell` ADDED 跳到底部提示与新消息计数;`设计规范/03` C14、`02` Ctrl+End
- 验证:`cargo test --lib` 357 passed(含 mouse-wheel);`clippy` 零警告;`validate --strict` 过

## 待决

- 内联渲染(epic D)若落地,本 change 的 in-app 滚动跟踪 / jump-to-bottom 将作废(终端接管滚动)—— 届时 OpenSpec REMOVED

## 引用

- change:`add-jump-to-bottom-indicator`(archive `changes/archive/2026-06-29-add-jump-to-bottom-indicator`)
- 关联:`add-input-history-and-permission-modes`(32,↑↓ 改历史→滚动靠 Page/Home 才催生本 pill 需求)、`enable-mouse-wheel-scroll`(34)
- session 主导:用户实测反馈 → AskUserQuestion 定(文案中/英、计数口径)→ 子 agent 实现 → 主 agent 复核揪 per-round 假绿 → per-turn 修复复核通过
