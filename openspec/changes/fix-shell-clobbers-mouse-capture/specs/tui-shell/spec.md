# tui-shell Delta

## MODIFIED Requirements

### Requirement: 键盘滚动全覆盖与鼠标滚轮降级(ConPTY 限制)

键盘 SHALL 提供 transcript 的导航且**不依赖**鼠标捕获:整页(`PageUp` / `PageDown`)、到顶(`Ctrl+Home`)、回底并恢复跟随(`Ctrl+End`)合起来 MUST 能从任意位置到达 transcript 的**顶**与**底**。**`↑` / `↓` 不用于 transcript 滚动**——在主输入态改为多行光标 / 输入历史导航(见「输入历史 ↑↓ 召回」);**裸 `Home` / `End` 不用于 transcript 滚动**——改为输入行内光标(见「多行输入编辑」)。故纯键盘滚动以**页级 + 边界**(`PageUp` / `PageDown` / `Ctrl+Home` / `Ctrl+End`)覆盖到顶 / 底;**不再保证逐行键盘滚动**,逐行仅在转发滚轮的终端经鼠标滚轮提供(ConPTY 无滚轮时只能页级遍历——此为在 ↑↓ / Home-End 抢键冲突上的取舍)。鼠标滚轮(`MouseEventKind::ScrollUp` / `ScrollDown`)SHALL 作为**尽力而为**的增强:在转发滚轮事件的终端可用;在 **ConPTY 不转发滚轮的 Windows 构建**上滚轮事件不到达 crossterm,此失效 MUST NOT 削弱键盘的页级 + 边界覆盖(滚轮缺失时仍可纯键盘到达顶 / 底)。`scroll_up` / `scroll_down` 原语 MUST 保留供页级实现与滚轮复用。`terminal.rs` 的鼠标捕获 MUST 保持开启(失效根因在平台而非捕获缺失,不因本 change 关闭捕获)。

**捕获重申(子进程冲击恢复)**:子进程 attach console 可能重置输入模式,使终端把滚轮**降级为 ↑/↓ 方向键**(表现为滚轮失效、且方向键误入多行光标 / 输入历史路径)。事件循环 SHALL 在 ui_rx 处理完 `AgentEvent::ToolCallFinished` 后幂等重发 `EnableMouseCapture`(`TerminalGuard::reassert_mouse_capture()`);重申对已开启状态无副作用。根因侧的子进程 console 独立性见 builtin-tools「run_shell 执行」。

#### Scenario: 纯键盘到顶与回底(无任何 MouseEvent)

- **WHEN** transcript 行数超视口、跟随底部态,**不**投入任何 `Event::Mouse`,仅以键盘调 `scroll_to_top`(`Ctrl+Home`)再 `scroll_to_bottom`(`Ctrl+End`)
- **THEN** `scroll_to_top` 后 `visible_scroll_offset` 指向顶(0)、`follows_bottom` 为假;`scroll_to_bottom` 后 `follows_bottom` 为真且回到底部偏移

#### Scenario: ↑ / ↓ 与裸 Home / End 不再滚 transcript

- **WHEN** 主输入态(无浮层)按 `↑` 或裸 `Home`
- **THEN** transcript 滚动位置不变;`↑` 归多行光标/输入历史、裸 `Home` 归输入行首光标
- **WHEN** 需要键盘滚 transcript 到顶
- **THEN** 用 `Ctrl+Home`(到顶)或 `PageUp`(页级),`↑` / `↓` / 裸 `Home` / `End` 不参与滚动
