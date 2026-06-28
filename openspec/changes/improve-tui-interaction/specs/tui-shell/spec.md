## ADDED Requirements

### Requirement: 工具输出折叠与全局展开(ctrl+o)

`AppState` SHALL 持折叠态 `tools_expanded: bool`(默认 `false` = 折叠)。`render` MUST 据 `tools_expanded` 对每个 `TranscriptBlock::Tool(ToolCard)` 块二选一渲染:**折叠**态仅渲染 `设计规范/03` C5 的**单行头**(状态 glyph + 工具名 + args 摘要 + 结果摘要),**不**渲染 output 体与脚;**展开**态渲染**全量**(头 / 体 / 脚 + 截断标记,即现状)。折叠行结果摘要 SHALL 为:`done` 且 output 非空 → ` · {N} 行 ⌄`(N = output 行数,`⌄` 提示可展开);携 `exit` → ` · exit {code}`(非 0 用 `error.fg`);`running` → ` · 运行中…`。`ctrl+o`(`KeyCode::Char('o')` + `KeyModifiers::CONTROL`,**仅** `KeyEventKind::Press`)MUST 翻转 `tools_expanded`(全局展开/折叠**所有**工具卡),且 MUST NOT 把 `o` 写入输入框(在文本输入 arm 之前拦截)。折叠**仅作用于** `Tool` 块;`User` / `Assistant` / `Error` / `Help` / `Status` / `Notice` 块 MUST NOT 受折叠影响。本期**只**提供全局 toggle,不提供单条卡片独立展开。

#### Scenario: 工具卡默认折叠为单行(带色快照)

- **WHEN** 一个 `done` 且 output 多行的 `Tool` 块在 `tools_expanded == false`(默认)下渲染到 `TestBackend`
- **THEN** 带色快照仅含该卡的单行头(glyph + 工具名 + args + ` · {N} 行 ⌄`),**不**含 output 体行与 `└─` 脚,与锁定一致

#### Scenario: ctrl+o 全局展开再折回(逻辑 + 快照)

- **WHEN** 对含若干 `Tool` 块的 `AppState` 投入 `ctrl+o`(`Char('o')`+`CONTROL`,`Press`)一次,再投一次
- **THEN** 第一次后 `tools_expanded == true` 且所有 `Tool` 块渲为全量(头 + 体 + 脚 + 截断标记);第二次后 `tools_expanded == false` 折回单行;两态带色快照各与锁定一致

#### Scenario: ctrl+o 仅 Press 且不写入输入框

- **WHEN** 依次投入 `ctrl+o` 的 `Release` 与 `Repeat`,再投入 `Press`
- **THEN** 仅 `Press` 翻转 `tools_expanded`;`Release` / `Repeat` 不翻转;任一情况下输入框串均不出现字符 `o`

#### Scenario: 折叠仅作用于 Tool 块

- **WHEN** transcript 含 `User` / `Assistant` / `Tool`(done)三块且 `tools_expanded == false` 时渲染
- **THEN** `User` / `Assistant` 块仍**全文**渲染(不折叠),仅 `Tool` 块折为单行,与锁定带色快照一致

### Requirement: 键盘滚动全覆盖与鼠标滚轮降级(ConPTY 限制)

键盘 SHALL 提供 transcript 的**完整**导航且**不依赖**鼠标捕获:行级(`↑` / `↓`)、整页(`PageUp` / `PageDown`)、到顶(`Home`)、回底并恢复跟随(`End`)合起来 MUST 能从任意位置到达 transcript 的**顶**与**底**。鼠标滚轮(`MouseEventKind::ScrollUp` / `ScrollDown`)SHALL 作为**尽力而为**的增强:在转发滚轮事件的终端(多数 Unix 终端、较新 Windows 11 conhost)可用;在 **ConPTY 不转发滚轮的 Windows 构建**(平台限制,见 design.md 决策 ③ 与 `microsoft/terminal` #376 / #545)上滚轮事件不到达 crossterm,此失效 MUST NOT 削弱键盘全覆盖(滚轮缺失时仍可纯键盘遍历全文)。行级滚动键与鼠标滚轮 MUST 复用同一 `scroll_up` / `scroll_down` 原语,使键盘可达滚轮所能达的任意滚动位置(键盘为滚轮能力的超集)。`terminal.rs` 的鼠标捕获 MUST 保持开启(失效根因在平台而非捕获缺失,不因本 change 关闭捕获)。

#### Scenario: 纯键盘到顶与回底(无任何 MouseEvent)

- **WHEN** transcript 行数超视口、处于跟随底部态,**不**投入任何 `Event::Mouse`,仅以键盘调用 `scroll_to_top` 再 `scroll_to_bottom`
- **THEN** `scroll_to_top` 后 `visible_scroll_offset` 指向顶(0)、`follows_bottom` 为假;`scroll_to_bottom` 后 `follows_bottom` 为真且 `visible_scroll_offset` 回到底部偏移

#### Scenario: 滚轮缺失无能力损失(行级键 == 滚轮)

- **WHEN** 以键盘 `↑`(行级,`scroll_up` 1 行)上滚 K 次,与「等量鼠标滚轮上滚」对照
- **THEN** 二者皆经同一 `scroll_up` 原语改变 `scroll_offset`,到达相同偏移;即使滚轮事件永不到达,键盘行级步进仍可复现任意滚动位置

### Requirement: 诊断事件日志(env 门控)

TUI SHALL 提供环境变量门控的事件诊断日志:当 `MYSTERIES_TUI_DEBUG_EVENTS` 被设置且非空时,`run_tui` SHOULD 将 crossterm `Event` 的脱敏摘要追加写入 `std::env::temp_dir()` 下固定文件名(如 `mysteries-tui-events.log`),用于真机核验滚轮事件是否到达。日志写入失败 MUST 静默降级,不得中断主循环或改变 TUI 交互语义。核心格式化函数 `debug_event_line(&Event) -> String` MUST 是纯函数、可单测、输出确定。诊断日志 MUST NOT 记录凭据、prompt 正文、配置路径、cwd 或其它用户文件内容;`KeyCode::Char` 的具体字符 MUST 脱敏,只保留事件类别 / key kind / modifiers / mouse kind 等定位滚轮所需元数据。

#### Scenario: 已知 Event 生成确定诊断行

- **WHEN** 调用 `debug_event_line(&Event::Mouse(MouseEventKind::ScrollUp, ...))` 与 `debug_event_line(&Event::Key(KeyCode::Char('x') + CONTROL, Press))`
- **THEN** 输出行确定且包含事件类别、kind / modifiers 等结构信息;Key 行不包含字符 `x`,避免把用户输入正文写入日志

## MODIFIED Requirements

### Requirement: transcript 滚动

`AppState` SHALL 维护 transcript 的 `scroll_offset`:默认**跟随底部**(新内容自动到底);手动滚动支持 **PageUp / PageDown**(整页)、**`↑` / `↓`(行级步进,复用 `scroll_up` / `scroll_down`,每次 1 行)**、**`Home`(`scroll_to_top`,到顶)**、**`End`(`scroll_to_bottom`,回底并恢复底部跟随)**;**鼠标滚轮**(`MouseEventKind::ScrollUp` / `ScrollDown`)经行级步进滚动(默认每次 N 行)。`scroll_to_top` MUST 置 `scroll_offset = 0` 且 `follows_bottom = false`;`scroll_to_bottom` MUST 置 `follows_bottom = true`(下一帧贴底)。滚到非底部时新内容 MUST NOT 强制拉回底部(保持阅读位置);滚回底部时 MUST 恢复跟随;offset MUST clamp 在 [顶, 底](不越界)。**仅 transcript 滚动**,顶栏 / 状态行 / 输入框 / 权限框固定。所有滚动键处理 SHALL 仅响应 `KeyEventKind::Press`(忽略 Release / Repeat)。鼠标滚轮要求终端 guard 进入时启用、退出 / panic 时关闭鼠标捕获。offset / 跟随逻辑 MUST 可单测。

#### Scenario: 跟随、手动滚、clamp(逻辑可测)

- **WHEN** 在底部时追加新内容 → 仍贴底;PageUp 后追加新内容 → 保持当前位置(不回底);PageUp/PageDown 至边界 → offset clamp 不越顶 / 底
- **THEN** `scroll_offset` 按上述规则变化(纯逻辑断言)

#### Scenario: 行级 / 鼠标滚轮步进与触底恢复跟随(逻辑可测)

- **WHEN** 调 `scroll_up`(行级)上滚若干行,再 `scroll_down` 步进直至触底
- **THEN** 上滚后 `follows_bottom` 为假且 offset 按行级步进变化;触底后 `follows_bottom` 恢复为真(后续新内容再次贴底)

#### Scenario: Home 到顶 / End 回底恢复跟随(逻辑可测)

- **WHEN** 在跟随底部态调 `scroll_to_top`,随后调 `scroll_to_bottom`
- **THEN** `scroll_to_top` 后 `scroll_offset == 0` 且 `follows_bottom == false`(`visible_scroll_offset` 指向顶);`scroll_to_bottom` 后 `follows_bottom == true` 且 `visible_scroll_offset` 回到底部偏移

#### Scenario: 滚动后的 transcript 快照

- **WHEN** transcript 行数超视口且 `scroll_offset` 指向中段时渲染
- **THEN** 快照只显对应窗口的 transcript 行,顶栏 / 状态行 / 输入框位置不变
