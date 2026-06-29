## 1. 新消息计数(纯函数单测)

- [x] 1.1 `AppState` += `new_message_count`;助手消息**完成**时若**未跟随底部**则 +1 的纯逻辑 + 单测:助手 done → +1;工具卡 / user 回显 / notice → 不计;follows-bottom 转真(回底)→ 清零;跟随底部时助手消息到达不累加。
- [x] 1.2 接线:在助手块完成的事件处理处调用计数增量;回底(`scroll_to_bottom` / follows-bottom 转真)处清零。先核既有跟随态是持久标志还是渲染期推导,缺标志则补一个由滚动动作维护的 bool。

## 2. Ctrl+End 回底

- [x] 2.1 确认 `Ctrl+End` 经 `scroll_action_for_key` 命中 `scroll_to_bottom`(`KeyCode::End` 按 code 匹配,modifier-agnostic);加测试断言 `End` 与 `Ctrl+End` 均映射 `scroll_to_bottom`,回底后计数清零。

## 3. 渲染(事后 insta)

- [x] 3.1 `render_jump_to_bottom_pill`:**未跟随底部**时在 `layout_rows[1]` 视口底部一行**局部** `Clear`(仅 pill 宽)+ 渲染两态文案(`跳到底部 (ctrl+End) ↓` / `N 条新消息 (ctrl+End) ↓`),theme 配色 + `↓` glyph;跟随底部不渲染。
- [x] 3.2 insta 快照三态:跟随底部(无 pill)/ 滚离底部无新消息(`跳到底部`)/ 滚离底部 N 条(`N 条新消息`);确认局部 Clear 无全宽黑带。

## 4. 设计规范 + 校验

- [x] 4.1 `设计规范/03-组件清单.md` 新增 C14 · 跳到底部 pill;`设计规范/02-布局与交互.md` 补 `Ctrl+End` 键位。
- [x] 4.2 `cargo test --lib` 全绿 + `cargo clippy --all-targets -- -D warnings` 零警告 + `openspec validate add-jump-to-bottom-indicator --strict` 过。
