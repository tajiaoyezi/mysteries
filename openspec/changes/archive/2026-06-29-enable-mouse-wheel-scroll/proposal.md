## Why

`add-input-history-and-permission-modes` 把 `↑↓` 改归输入历史后,实测发现:鼠标捕获关闭时,Windows Terminal 在 alternate screen 下走 **alternate-scroll-mode**,把**鼠标滚轮翻译成 `↑/↓` 方向键**送进程序 → 滚轮误触输入历史。滚轮与键盘 `↑↓` 在该模式下是**同一种输入**,无法区分。要让滚轮滚动 transcript、`↑↓` 专归历史,**必须启用鼠标捕获**(使滚轮以 `Event::Mouse` 到达,而非方向键)。

## What Changes

- **`terminal.rs` 启用鼠标捕获**:`setup` 发 `EnableMouseCapture`,退出 / panic 经 `restore_terminal` 发 `DisableMouseCapture`。
- **事件循环处理滚轮**:`Event::Mouse(ScrollUp / ScrollDown)` → transcript 上 / 下滚动(复用 `scroll_up` / `scroll_down`,每事件固定行数);键盘 `↑↓` 不受影响(仍归历史)。
- **反转「终端原生复制(不捕获鼠标)」**:现捕获鼠标;终端原生框选复制让位于 **Shift+拖选**。
- **需真机验证**:部分 Windows ConPTY 构建可能仍不转发滚轮事件;若如此滚轮无效(降级),但 MUST NOT 影响键盘滚动与 `↑↓` 历史。

## Capabilities

### New Capabilities

(无)

### Modified Capabilities

- `tui-shell`:**REMOVED**「终端原生复制(不捕获鼠标)」;**ADDED**「鼠标滚轮滚动(捕获鼠标)」(启用捕获、滚轮驱动 transcript 滚动、退出 / panic 解除捕获、原生复制改 Shift+拖)。

## Impact

- **代码**:
  - `src/tui/terminal.rs`:`EnterAlternateScreen` 处加 `EnableMouseCapture`;`restore_terminal`(`Drop` + panic hook 共用)加 `DisableMouseCapture`;import `crossterm::event::{EnableMouseCapture, DisableMouseCapture}`。
  - `src/tui/mod.rs`:`Event::Mouse(me)` 由忽略改为按 `me.kind`(`ScrollUp` / `ScrollDown`)经 `apply_scroll` 调 `scroll_up` / `scroll_down`(每事件固定行数);其余 mouse kind 忽略。映射抽小函数可单测。
  - `设计规范/02-布局与交互.md`:补滚轮滚动 + 复制改 Shift+拖说明。
- **依赖**:零新依赖(crossterm 已在)。
- **测试**:滚轮 kind → 滚动原语的映射纯函数单测;capture enable/disable 以 code 审(不易单测)。
- **设计规范偏差(port/adapt/drop)**:滚轮滚动 = port 行为;原生复制 → Shift+拖 = 文档说明(drop 原生捕获豁免)。

## 风险 / 验证

启用捕获是否真能让滚轮以 `Event::Mouse` 到达,取决于终端 / ConPTY 是否转发滚轮——**须用户在 WT 上实测**:滚轮能否滚动 transcript、`↑↓` 是否仍翻历史、Shift+拖能否复制。若滚轮仍无效则回退讨论。
