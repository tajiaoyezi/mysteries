## Context

`add-input-history-and-permission-modes` 把 `↑↓` 从行级滚动改归输入历史后,transcript 滚动只剩 `PageUp`/`PageDown`/`Home`/`End`(+ 滚轮)。用户实测反馈:滚上去后看不出是否在最新输出位置,也不知滚走期间模型答了几条。现有滚动系统(`src/tui/app.rs`):`scroll_offset` + `visible_scroll_offset(total, viewport)` + `scroll_to_bottom`/`scroll_to_top` + follows-bottom 跟随语义;transcript 视口 = `layout_rows[1]`。`Ctrl+End` 经 crossterm 给 `KeyCode::End` + `CONTROL`,`scroll_action_for_key` 按 `key.code` 匹配,已命中 `scroll_to_bottom`。

## Goals / Non-Goals

**Goals:**
- 未跟随底部时,视口底部钉一条 pill:`跳到底部 (ctrl+End) ↓` 或 `N 条新消息 (ctrl+End) ↓`。
- 新消息计数 = 已完成助手消息(一轮 = 1),未跟随底部期间累加、回底清零。
- `Ctrl+End` 回底 + 恢复跟随 + 清零。

**Non-Goals:**
- 不改 `↑↓`(归 `add-input-history-and-permission-modes`)。
- 不做「未读分隔线」、不持久化计数、不改滚轮处理。

## Decisions

- **D1 pill 仅在「未跟随底部」渲染,钉视口底部。** `render_jump_to_bottom_pill` 在 `layout_rows[1]` 底部一行**局部覆盖**(`Clear` 仅 pill 宽 —— 沿用 C12 picker「全宽 Clear 致黑带」的教训);跟随底部时完全不渲染。

- **D2 计数 = 已完成助手消息,AppState 维护 `new_message_count`(纯逻辑)。** 助手消息**完成**(`TextDone` / 助手块 done)时,若当前**未跟随底部**则 `new_message_count += 1`;`user` 回显 / 工具卡 / `notice` 一律不计;follows-bottom 转真(回底)时清零。增量函数纯、可单测(覆盖:助手 +1、工具卡不 +、回底清零)。流式中途不计,只在助手块 done 时 +1。

- **D3 计数时机用 AppState 维护的 follows-bottom 标志,不依赖渲染期 dims。** 计数增量发生在**消息到达**(事件处理)时,此刻无 viewport 尺寸;故复用 AppState 在滚动键里维护的 follows-bottom 跟随状态判断,而非渲染期由 `total/viewport` 重算。实现期以 code 为准对齐既有跟随机制(若现有跟随是渲染期推导而无持久标志,则补一个 bool 标志由滚动动作维护)。

- **D4 `Ctrl+End` 复用既有 `KeyCode::End` 路由。** `scroll_action_for_key` 按 `code` 匹配,`Ctrl+End` 与 `End` 同走 `scroll_to_bottom`;回底由 follows-bottom 转真触发清零,与具体按键无关。pill hint 标 `ctrl+End`(`End` 亦可)。

- **D5 文案中文 + 仅助手计数(用户拍板)。** `跳到底部 (ctrl+End) ↓` / `N 条新消息 (ctrl+End) ↓`;技术词 `ctrl+End` 保留英文。

- **D6 样式 = adapt,新增设计规范 C14。** theme pill(`bg.surface` 底 + 描边或 `accent` 强调),`↓` glyph;`设计规范/03` 加 C14、`02` 补 `Ctrl+End` 键位。事后 insta 快照。

## Risks / Trade-offs

- **follows-bottom 判定时机**:计数在事件处理期增量,需此刻知跟随态 → 依赖 AppState 持久 follows-bottom 标志(滚动动作维护),不可用渲染期重算。实现须先核既有跟随是标志还是推导(code > spec)。
- **pill 覆盖 transcript 末行**:局部 `Clear`(仅 pill 宽)避免全宽黑带(C12 教训);pill 遮住的那一行 transcript 内容在回底后即恢复可见。
- **计数语义边界**:只在助手块 **done** +1(流式中途不计),避免一条消息边流边累加;工具卡多不影响计数。
- **与 `add-input-history-and-permission-modes` 的次序**:本 change 纯 ADDED,不重复 MODIFIED「键盘滚动」requirement;须待前者归档后再归档本 change。
