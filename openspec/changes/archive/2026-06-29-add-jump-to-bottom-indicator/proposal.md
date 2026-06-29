## Why

`↑↓` 改归输入历史后,transcript 滚动只剩 `PageUp` / `Home` 等;用户滚上去后(实测反馈)**看不出自己是否在模型最新输出的位置**,也不知道滚走期间模型又答了几条。需要一个类 Claude Code 的「跳到底部」可视提示 + 新消息计数。

## What Changes

- transcript **未跟随底部**时,在视口底部、输入框上方钉一条单行 pill;**跟随底部时隐藏**。
- pill 两态文案(中文):无新增助手消息 → `跳到底部 (ctrl+End) ↓`;滚走后又新增 N 条 → `N 条新消息 (ctrl+End) ↓`。
- **新消息计数**:只计**已完成的助手消息**(一轮答复 = 1),不计 user 回显 / 工具卡 / notice;未跟随底部期间累加,回到底部清零。
- `Ctrl+End` → transcript 回底 + 恢复跟随 + 计数清零(`End` 亦可)。

## Capabilities

### New Capabilities

(无)

### Modified Capabilities

- `tui-shell`:**ADDED** 跳到底部 pill(未跟随底部时渲染、两态文案、`Ctrl+End` 回底、仅助手消息计数)。

## Impact

- **代码**:
  - `src/tui/app.rs`:`AppState` += `new_message_count`;助手消息完成时若未跟随底部则 +1、回底清零(纯逻辑,可单测);复用既有 `follows_bottom` / `scroll_to_bottom`。
  - `src/tui/mod.rs`:确认 `Ctrl+End` 路由到 `scroll_to_bottom`(`KeyCode::End` 按 code 匹配,已 modifier-agnostic 命中)。
  - `src/tui/render.rs`:`render_jump_to_bottom_pill` —— 未跟随底部时在 `layout_rows[1]` 视口底部局部渲染(`Clear` 仅 pill 宽,避黑带),两态文案,`↓` glyph,theme 配色。
  - `设计规范/03-组件清单.md`:新增 C14 · 跳到底部 pill;`设计规范/02`:补 `Ctrl+End` 键位。
- **依赖**:零新依赖。
- **测试**:计数逻辑纯函数单测(助手计数、工具卡/user 不计、回底清零);pill 渲染 insta 快照(三态);`Ctrl+End` 路由测试。
- **设计规范偏差(port/adapt/drop)**:pill = **adapt**(Claude 风格,配色用本项目 theme token,新增 C14);`↓` glyph = port。

## 前置

- 依赖 `add-input-history-and-permission-modes` 先归档(该 change 已把 `↑↓` 从滚动移除、滚动落到 `PageUp`/`Home`/`End`)。本 change 纯 **ADDED**,不再触碰其 MODIFIED 的「键盘滚动」requirement,避免归档冲突。
