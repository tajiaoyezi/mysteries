## ADDED Requirements

### Requirement: 输入历史 ↑↓ 召回(本会话内存)

系统 SHALL 在**主输入态**(无浮层:无 pending 权限框、无 `/` 命令补全、无 models picker)下提供输入历史:`↑` 召回上一条、`↓` 召回下一条**已提交输入**(普通 prompt 与 `/命令` 均入历史)。进入历史前的草稿 MUST 被保存,游标越过最新一条时 `↓` SHALL 恢复该草稿;在历史某条上**键入字符**或**提交** MUST 重置游标回草稿态。连续两次提交相同文本 MUST 只入历史一条。历史仅存于本会话内存,关闭 TUI 即清空(不落盘)。历史导航为纯函数 reducer,可单测。命令补全 / picker 打开时 `↑↓` 归各自浮层处理,历史不参与。

#### Scenario: ↑ 逐条回溯、↓ 前进

- **WHEN** 依次提交 `a`、`b`,在空输入态按 `↑`
- **THEN** 输入框为 `b`;再按 `↑` → `a`
- **WHEN** 在 `a` 上按 `↓`
- **THEN** 输入框回到 `b`

#### Scenario: ↓ 越过最新恢复草稿

- **WHEN** 输入框已键入未提交草稿 `dr`,按 `↑` 进入历史,再按 `↓` 越过最新一条
- **THEN** 输入框恢复为 `dr`

#### Scenario: 键入字符脱离历史

- **WHEN** 处于历史某条,键入一个字符
- **THEN** 游标回草稿态,该字符追加到当前文本,后续 `↑` 从最新条重新回溯

#### Scenario: 连续重复提交去重

- **WHEN** 连续两次提交相同文本 `x`
- **THEN** 历史中只保留一条 `x`

#### Scenario: 浮层打开时 ↑↓ 不归历史

- **WHEN** `/` 命令补全浮层打开,按 `↑↓`
- **THEN** 由补全浮层处理高亮移动,输入历史不变

### Requirement: 权限模式切换键与底部模式行

系统 SHALL 支持 `Shift+Tab`(`KeyCode::BackTab`)在 `Normal → AcceptEdits → Yolo → Normal` 间循环切换当前权限模式;切换 MUST 即时生效于后续工具决策(经共享模式句柄,与 agent-task 同一来源)。当前模式 SHALL 显示在**状态行下方一条独立的底部模式行**(屏幕最末行,**不**再占用状态行 C10),格式 `<glyph> <mode> · shift+tab 切换`,每模式带专属 glyph(`▸` Normal / `▸▸` AcceptEdits / `▲` Yolo)与 theme 配色(Normal `text.muted` / AcceptEdits `accent.primary` / Yolo `warning.fg`)。模式行 SHALL 常驻显示(含 `Normal`),令 `shift+tab` 提示可发现。状态行 C10 MUST NOT 再含模式段。切换键 SHALL 在任意 phase 可用(含 pending 权限框展示时,语义同 `ctrl+o`)。模式默认 `Normal`,不跨重启持久化。自动放行命中时不产生 pending 权限框(C6 不渲染),工具直接执行。

#### Scenario: Shift+Tab 循环切换

- **WHEN** 当前 `Normal`,按 `Shift+Tab`
- **THEN** 切到 `AcceptEdits`;再按 → `Yolo`;再按 → `Normal`

#### Scenario: 底部模式行反映当前模式

- **WHEN** 切到 `Yolo`
- **THEN** 屏幕最末行渲染 `▲ yolo · shift+tab 切换`(`warning.fg` 色),且状态行 C10 不含任何模式段

#### Scenario: Yolo 下改动类工具不弹权限框

- **WHEN** 当前 `Yolo`,模型调用一个 `Execute`(shell)工具
- **THEN** 不产生 pending 权限框(C6 不渲染),工具直接执行

## MODIFIED Requirements

### Requirement: 键盘滚动全覆盖与鼠标滚轮降级(ConPTY 限制)

键盘 SHALL 提供 transcript 的导航且**不依赖**鼠标捕获:整页(`PageUp` / `PageDown`)、到顶(`Home`)、回底并恢复跟随(`End`)合起来 MUST 能从任意位置到达 transcript 的**顶**与**底**。**`↑` / `↓` 不再用于 transcript 行级滚动**——在主输入态(无浮层)它们改为输入历史导航(见「输入历史 ↑↓ 召回」)。故纯键盘滚动以**页级 + 边界**(`PageUp` / `PageDown` / `Home` / `End`)覆盖到顶 / 底;**不再保证逐行键盘滚动**,逐行仅在转发滚轮的终端经鼠标滚轮提供(ConPTY 无滚轮时只能页级遍历——此为本 change 在 ↑↓ 抢键冲突上的取舍)。鼠标滚轮(`MouseEventKind::ScrollUp` / `ScrollDown`)SHALL 作为**尽力而为**的增强:在转发滚轮事件的终端可用;在 **ConPTY 不转发滚轮的 Windows 构建**上滚轮事件不到达 crossterm,此失效 MUST NOT 削弱键盘的页级 + 边界覆盖(滚轮缺失时仍可纯键盘到达顶 / 底)。`scroll_up` / `scroll_down` 原语 MUST 保留供页级实现与滚轮复用。`terminal.rs` 的鼠标捕获 MUST 保持开启(失效根因在平台而非捕获缺失,不因本 change 关闭捕获)。

#### Scenario: 纯键盘到顶与回底(无任何 MouseEvent)

- **WHEN** transcript 行数超视口、处于跟随底部态,**不**投入任何 `Event::Mouse`,仅以键盘调用 `scroll_to_top` 再 `scroll_to_bottom`
- **THEN** `scroll_to_top` 后 `visible_scroll_offset` 指向顶(0)、`follows_bottom` 为假;`scroll_to_bottom` 后 `follows_bottom` 为真且 `visible_scroll_offset` 回到底部偏移

#### Scenario: ↑ / ↓ 不再滚动 transcript 而归输入历史

- **WHEN** 主输入态(无浮层)按 `↑`
- **THEN** transcript 滚动位置不变(`↑` 非滚动键),改为召回上一条输入历史
- **WHEN** 需要键盘上滚 transcript
- **THEN** 用 `PageUp`(页级)或 `Home`(到顶),`↑` / `↓` 不参与滚动
