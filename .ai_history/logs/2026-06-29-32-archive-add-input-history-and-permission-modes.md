# 2026-06-29 · 32 · archive add-input-history-and-permission-modes

## 决策

- **输入历史 ↑↓ + permission 模式合一个 change** | 主导:用户(「这两个合并为一个 change」)| 取舍:省一轮 propose/archive,但一条 change 横跨 `tui-shell` / `permission-gate` / `tool-system` 三 spec、两套 TDD 规格(权限走严格红绿、历史走纯函数单测)—— 用户知情拍板
- **D1 三档 + 纯函数 `auto_allows(mode,level)`(headless 强制 TDD)** | `Normal`→非 ReadOnly 全询问;`AcceptEdits`→`Edit` 放行 `Execute` 询问;`Yolo`→全放行 | 主导:用户(AskUserQuestion 选「三档 + accept 仅放行编辑」)
- **D2 `PermissionLevel` 细分 `{ReadOnly, Edit, Execute}`** | 选:工具自身声明类别(类型安全,编译器强制每工具选 variant)| 弃:decider 按工具名 allow-list(脆弱、新工具漏配静默失效)、保留 `RequiresConfirmation` 加平行 `category()`(冗余轴)| `RequiresConfirmation` 从 src 全删
- **D3 模式态归 `ChannelDecider` 持 `Arc<Mutex<PermissionMode>>`,`gate()` 签名不变** | 模式是 TUI 运行时态(Shift+Tab 实时切),decider 在 TUI 层是天然 owner;`gate()` 在 headless agent loop 不加 mode 参数。`run_tui` 建**单一** Arc → clone 给 decider + AppState(共享,切换对真实决策有效)。`decide()` 快照模式即 drop guard(不跨 await 持锁)| 主导:讨论收敛 | 弃:`gate(mode,…)`(运行时 UI 态下推 headless 门)
- **D4 Shift+Tab(`BackTab`)在 `ctrl+o` 同档、浮层之前** | 任意 phase(含 pending)可切;切换序列纯逻辑单测
- **D5 输入历史 = 纯 reducer + 本会话内存** | 主导:用户(AskUserQuestion 选「本会话内存 MVP」)。`InputHistoryState` + `reduce_input_history`(`HistoryUp`/`Down`/`InsertChar`/`Backspace`/`PushSubmitted`);单一真源(移除 `AppState.input` 平行字段,嵌 `input_line` + `input()` 访问器);连续重复去重、空串不入
- **D6 模式 UI = 独立底部模式行(非状态行段)** | 主导:用户(实测嫌状态行挤、要求单起一行类 Claude Code;AskUserQuestion 选「双箭头+三角·中文提示」)| `▸`/`▸▸`/`▲` glyph(width-1,避 emoji)+ `text.muted`/`accent.primary`/`warning.fg` + `· shift+tab 切换`;状态行去 MODE 段
- **↑↓ 实机 bug(用户报)→ 根因在事件循环 seam** | 现象:↑↓ 滚 transcript 不翻历史。根因:`mod.rs` 事件循环先 `handle_scroll_key` 把 ↑↓ 当行滚动吃掉,到不了 `on_key` 历史 reducer;**单测直调 `on_key` 绕过了这层**(测试绿、实机坏)。修:`scroll_action_for_key` 删 Up/Down 两臂 → ↑↓ 落 on_key 历史;补事件循环级回归测试(断言 ↑↓ 非滚动键)。与既有「键盘滚动全覆盖」MODIFIED 显式解冲突(↑↓ 退出行滚动,滚动降页级+边界;Option A 用户拍板,ConPTY 失逐行键盘滚)
- **审查(主 agent 独立 cargo/clippy + 读码 + 4-agent 只读 workflow adversarial 复核)**:查实共享 Arc 真共享、gating 测试真断言 state-unchanged(非仅不 panic)、去重仅连续、decide 不跨 await 持锁;**两次栽在"测试停半路假绿"**(missing-style + ↑↓ 事件循环 seom + 后续 jump-to-bottom 计数 per-round)——均补完整路径测试

## 变更

- `src/permission/mod.rs`:`PermissionMode` + `auto_allows` + `cycle_permission_mode` + `permission_mode_label`;`gate()` 按 `{ReadOnly,Edit,Execute}` 派发
- `src/tool/{mod,edit,shell}.rs`:`PermissionLevel` 细分;edit→Edit、shell→Execute
- `src/tui/input_history.rs`(新):reducer + 6 单测;`app.rs`:`input_line` 嵌入 + `↑↓`/Backspace 接入 + BackTab 切模式 + 共享 mode Arc + gating 集成测
- `src/tui/channel.rs`:`ChannelDecider` 持共享 mode,`decide()` auto_allows 短路;`mod.rs`:共享 mode Arc 注入 + `scroll_action_for_key` 删 Up/Down(↑↓ 归历史);`render.rs`:底部 MODE 行 + 状态行去段
- spec:`permission-gate` ADDED 权限模式 + MODIFIED 门;`tool-system` MODIFIED 工具抽象(level 细分);`tui-shell` ADDED 输入历史 + 模式行 + MODIFIED 键盘滚动(↑↓ 退滚动)
- 验证:`cargo test --lib` 356 passed;`clippy --all-targets -D warnings` 零警告;`validate --strict` 过

## 待决

- 多行输入(路线图;落地时 ↑↓ 改「首行才翻历史」)
- `complete_selected_command` 直写 `input_line.input` 绕 reducer(当前无害,minor)
- `HistoryUp` 仅非空 input 存 draft(边角:退格清空后旧 draft 复现,minor)

## 引用

- change:`add-input-history-and-permission-modes`(D1–D6 见 design.md;archive `changes/archive/2026-06-29-add-input-history-and-permission-modes`)
- 后续:`add-jump-to-bottom-indicator`(33)、`enable-mouse-wheel-scroll`(34)—— 三条 entangled,src 合一个 feat 提交(`5534a4d`)、分三条 chore archive
- session 主导:用户拆需求 → AskUserQuestion 定档(模式范围/切换键/历史存储/模式行样式/↑↓ 归属)→ 子 agent 红灯停点实现 → 主 agent 独立复核(cargo/clippy + 读码 + 4-agent workflow)→ 实机 bug(↑↓ 滚动 seam)修复
