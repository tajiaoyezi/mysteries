## REMOVED Requirements

### Requirement: 终端原生复制(不捕获鼠标)

**Reason**: `↑↓` 改归输入历史后,鼠标捕获关闭时 Windows Terminal 的 alternate-scroll-mode 把鼠标滚轮翻译成 `↑/↓` 方向键 → 误触输入历史;滚轮与键盘 `↑↓` 无法区分。为分离二者,改为**启用**鼠标捕获(见新增「鼠标滚轮滚动(捕获鼠标)」)。
**Migration**: 终端原生框选复制改用 **Shift+拖选**(多数终端在捕获态仍支持);程序内滚轮事件改由 `Event::Mouse` 驱动 transcript 滚动。键盘滚动(`PageUp` / `PageDown` / `Home` / `End`)与 `↑↓` 历史不变。

## ADDED Requirements

### Requirement: 鼠标滚轮滚动(捕获鼠标)

TUI SHALL 启用鼠标捕获:`TerminalGuard` 进入 alternate screen 时发 `EnableMouseCapture`,退出 / panic 时经 `restore_terminal` 发 `DisableMouseCapture`,使鼠标滚轮以 `Event::Mouse` 到达程序而非被终端翻译为 `↑/↓` 方向键。`MouseEventKind::ScrollUp` / `ScrollDown` SHALL 经 `scroll_up` / `scroll_down` 原语驱动 transcript 上 / 下滚动(每事件固定行数);其余 mouse kind MUST 被忽略(不改交互)。键盘 `↑↓` MUST NOT 受影响(仍归输入历史)。鼠标捕获 MUST 在退出 TUI / panic 时正确解除(沿用 `restore_terminal` 单一路径,不残留鼠标模式)。终端原生框选复制让位于 Shift+拖选。

**降级**:部分 Windows ConPTY 构建即便捕获也可能不转发滚轮事件——此时滚轮无效,但 MUST NOT 影响键盘滚动(`PageUp` / `PageDown` / `Home` / `End`)与 `↑↓` 历史(键盘全覆盖不受损)。

#### Scenario: 进入 TUI 启用鼠标捕获、退出解除

- **WHEN** 构造 `TerminalGuard` 进入 alternate screen
- **THEN** 终端被置入鼠标捕获模式(setup 发 `EnableMouseCapture`)
- **WHEN** TUI 退出或 panic,经 `restore_terminal`
- **THEN** `DisableMouseCapture` 被发出,终端恢复原生鼠标行为(无残留)

#### Scenario: 滚轮事件驱动 transcript 滚动

- **WHEN** 收到 `Event::Mouse` 且 kind 为 `ScrollUp`
- **THEN** 经 `scroll_up` 原语上滚 transcript(固定行数)
- **WHEN** 收到 `Event::Mouse` 且 kind 为 `ScrollDown`
- **THEN** 经 `scroll_down` 原语下滚 transcript

#### Scenario: 键盘 ↑↓ 不受滚轮捕获影响

- **WHEN** 鼠标捕获已启用,主输入态按键盘 `↑`
- **THEN** 仍召回输入历史(滚轮捕获不改键盘 `↑↓` 语义)
