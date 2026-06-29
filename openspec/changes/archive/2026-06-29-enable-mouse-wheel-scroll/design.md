## Context

`src/tui/terminal.rs`:`TerminalGuard::new` 仅 `enable_raw_mode` + `EnterAlternateScreen`(**无**鼠标捕获);`restore_terminal`(`Drop` 与 panic hook 共用)`disable_raw_mode` + `LeaveAlternateScreen`。`src/tui/mod.rs:146` `Event::Mouse(_) => {}` 忽略鼠标。已有滚动原语 `scroll_up` / `scroll_down(total, viewport, lines)` + `apply_scroll`(算 `total_lines` / `viewport`)。`add-input-history-and-permission-modes` 已把 `↑↓` 从滚动改归历史;实测:捕获关闭时 WT 把滚轮翻译成 `↑↓` → 误触历史(本 change 的动因)。

## Goals / Non-Goals

**Goals:**
- 启用鼠标捕获,使滚轮以 `Event::Mouse` 到达 → 驱动 transcript 滚动。
- 键盘 `↑↓` 仍归输入历史,与滚轮分离。
- 退出 / panic 正确解除捕获。

**Non-Goals:**
- 不改键盘滚动键、不改输入历史、不处理鼠标点击 / 拖拽 / 移动(只 wheel)。
- 不解决 ConPTY 可能不转发滚轮的平台限制(降级:滚轮无效但键盘不受损)。

## Decisions

- **D1 捕获生命周期复用单一 setup / restore 路径。** `setup`:`execute!(stdout, EnterAlternateScreen, EnableMouseCapture)`;`restore_terminal`:`execute!(stdout, DisableMouseCapture, LeaveAlternateScreen)`。`Drop` 与 panic hook 都走 `restore_terminal`,故捕获在正常退出与 panic 下均解除,不残留鼠标模式。

- **D2 滚轮 → 滚动映射抽小函数,可单测。** 事件循环 `Event::Mouse(me)`:`me.kind` 为 `ScrollUp` → `scroll_up`、`ScrollDown` → `scroll_down`,每事件固定 **3 行**(手感,可调);其余 kind → 忽略。经既有 `apply_scroll` 取 `total_lines` / `viewport`。映射(kind → 滚动原语 / 行数)抽纯函数单测,类比 `scroll_action_for_key`。

- **D3 反转 no-capture requirement(REMOVED + ADDED)。** 原「终端原生复制(不捕获鼠标)」整条 REMOVED(Reason / Migration 说明),新增「鼠标滚轮滚动(捕获鼠标)」。原生框选复制 → Shift+拖选(`设计规范/02` 注明)。

- **D4 真机验证为验收门。** 启用捕获能否让滚轮以 `Event::Mouse` 到达取决于 WT / ConPTY 转发;**用户实测**:滚轮滚动 transcript ✓ / `↑↓` 仍翻历史 ✓ / Shift+拖能复制 ✓。若滚轮仍无效(ConPTY 不转发)→ 反馈再议(回退捕获或保留仅键盘滚动)。

## Risks / Trade-offs

- **ConPTY 可能不转发滚轮**:即便捕获,部分 Windows 构建滚轮事件仍不到达 crossterm → 滚轮无效。降级安全:键盘滚动(`PageUp`/`PageDown`/`Home`/`End`)+ `↑↓` 历史不受影响。**须用户实测确认本 change 是否真修好滚轮。**
- **失去原生框选复制**:捕获态终端不再原生处理选择 → 改 Shift+拖选(多数终端支持)。这是用户在「滚轮分离 vs 原生复制」上的明确取舍(拍板选捕获)。
- **捕获泄漏风险**:若退出未解除,终端残留鼠标上报模式(乱码)。`restore_terminal` 单一路径 + panic hook 已覆盖正常 / 异常退出;实现须确保 `DisableMouseCapture` 在两路径都发。
- **每 notch 行数**:3 行为初值,手感不合可调;不影响正确性。
