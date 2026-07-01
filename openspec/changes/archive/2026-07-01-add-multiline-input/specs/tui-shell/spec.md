## ADDED Requirements

### Requirement: 多行输入编辑(文本缓冲 + 光标 + 换行)

输入框 SHALL 支持**手动多行编辑**。核心为**纯逻辑文本缓冲**(与 ratatui 解耦、可单测):`text: String`(可含 `\n`)+ `cursor: usize`(字节位,**MUST 落在 char 边界**)+ 输入历史(list / index / draft)。归约 `reduce_*(state, action) -> state`,动作含 `InsertChar` / `InsertNewline` / `Backspace` / `Delete` / `MoveLeft` / `MoveRight` / `MoveLineStart` / `MoveLineEnd` / `Up` / `Down` / `SetText` / `PushSubmitted`(**不含粘贴批量 `InsertStr`——粘贴防狂发属后续 change**)。行按 `\n` 切分;光标列以 `display_width` 计(宽字符感知)。`display_width` / `char_width` MUST 抽到中立模块(`tui/width.rs`)供缓冲 reducer 与 render 共用(纯逻辑不得反依赖 render 外壳)。

**换行 vs 提交**:`Ctrl+Enter`(`KeyCode::Enter` + `CONTROL`)/ `Shift+Enter`(`Enter` + `SHIFT`)/ `Ctrl+J`(`KeyCode::Char('j')` + `CONTROL`)SHALL 在光标处插入 `\n`;`Enter`(无 CONTROL/SHIFT)SHALL **提交**整段 `text`(trim 后为空则 no-op)。换行键判定 MUST 在通用 `KeyCode::Char(ch)` 插入分支**之前**;通用 `Char` 分支 MUST 过滤**带 CONTROL 且不带 ALT** 的字符(纯 `Ctrl+字符`,如 `Ctrl+J`,避免插入字面 `j`);**AltGr 合成字符(同时带 CONTROL+ALT)MUST 保留插入**(国际键盘)。设计 MUST NOT 依赖 kitty keyboard protocol(`PushKeyboardEnhancementFlags` 在 Windows 返 `Err`,会致 TUI 无法启动)、MUST NOT 改 `terminal.rs`。**命令解析仅单行**:提交时 `text` **含 `\n`** → 整段作 prompt(不解析命令);**不含 `\n`** 且以 `/` 起头 → 走 `parse_command`。

**光标导航**:`←` / `→` 按 char 边界步进(跨宽字符整步,不落字符中间);`Home` / `End`(无 CONTROL)到**当前行**首 / 尾;`Backspace` 删光标前一 char、`Delete` 删后一 char。`↑` / `↓` 由 app 投**合并动作** `Up` / `Down`,reducer 按 cursor **逻辑行**位置分派:cursor **不在首逻辑行**时 `Up` SHALL 上移一逻辑行、**不在末逻辑行**时 `Down` SHALL 下移一逻辑行(按 `display` 列对齐,落宽字符跨列中间取「**≤ 目标视觉列的最大 char 边界**」;不维护 goal-column);cursor 在**首逻辑行** `Up` / **末逻辑行** `Down` SHALL 翻输入历史(见「输入历史 ↑↓ 召回」)。**`Up`/`Down` 按逻辑行分派**:单条超宽逻辑行虽软换行成多显示行,其内部不逐显示行上下移(在该逻辑行即视为首/末行)——v1 限制。

**模态 vs 软浮层路由**:所有编辑/光标/换行键 MUST 落在既有守卫**之后**。**硬模态**(`pending_permission` / `models_picker`)活跃时,编辑/换行/光标键 MUST NOT 进入文本缓冲(归模态处理)。**软浮层 `command_completion`** 活跃时:`Char` / `Backspace` **仍 MUST 改文本缓冲并触发补全重过滤**(保持既有 `/` 补全「继续输入重新过滤」);`Up` / `Down` / `Tab` / `Enter` / `Esc` 归补全浮层;其余光标/换行键(`←→ Home End Delete` / 换行)在补全打开时 MUST NOT 破坏补全过滤(建议忽略或先关浮层再执行)。Tab 不插入 `\t`(仍归命令补全;无补全态 no-op)。

**渲染**:`render_input` SHALL 多行渲染 `text`;输入框内容高度 SHALL 随行数**动态、封顶**:`cap = clamp(屏高 - (顶栏3 + status_top_gap + permission_height + 活动1 + 状态1 + mode1 + input 边框2) - transcript_floor, 1, 绝对上限)`,`transcript_floor` 与 `rows[1] = Min(8)` 口径一致(=8);**cap MUST ≥ 1 内容行**(屏高极小时 input 保 1 行、必要时挤占 floor 而非算得 0)。超宽逻辑行 SHALL **软换行**;render MUST 做 **logical(逻辑行,列) → visual(显示行,列)** 映射,把终端光标 `set_cursor_position` 到 visual 坐标;**cursor 落逻辑行软换行边界(=某显示行满宽处)时 MUST 归入下一显示行行首**(唯一确定,供单测)。显示行 > cap 时框内滚动使**光标 visual 行**可见。缓冲归约、光标、宽字符列映射、命令仅单行判定、框高 cap、logical↔visual 映射 MUST 纯逻辑单测;多行渲染 + 光标 + 动态框高 + 软换行走 `insta` 快照。

#### Scenario: 换行键插入 `\n`、Enter 提交、命令仅单行(纯逻辑 + 接线)

- **WHEN** 光标在文本中,分别收到 `Enter+CONTROL`、`Enter+SHIFT`、`Char('j')+CONTROL`;另在含 `\n` 的多行 `text`(首行以 `/` 起)按无 modifier 的 `Enter`
- **THEN** 前三者在光标处插入 `\n`、不提交;含 `\n` 的整段作为 **prompt 提交**(不被 `parse_command` 当命令);单行且以 `/` 起头才走命令解析;`Ctrl+J` 不插入字面 `j`

#### Scenario: 光标导航与编辑(纯逻辑)

- **WHEN** 对含多行的 `text` 归约 `MoveRight`/`MoveLeft`/`Up`/`Down`/`MoveLineStart`/`MoveLineEnd`/`Backspace`/`Delete`
- **THEN** cursor 始终落 char 边界;`←→` 单 char 步进、`Home/End` 到当前行首/尾、`Up/Down` 到相邻逻辑行(多行内)、`Backspace/Delete` 删光标前/后一 char

#### Scenario: 宽字符光标与列对齐(纯逻辑,可判定 tie)

- **WHEN** 文本含 CJK 宽字符(如「你好」),做 `←→` 与跨行 `Up`/`Down`
- **THEN** `←→` 跨宽字符整步;跨行按 `display_width` 视觉列对齐,落宽字符跨列中间时取「≤ 目标列的最大 char 边界」,结果确定可测

#### Scenario: 单行 vs 多行时 Up/Down 分派(纯逻辑)

- **WHEN** 单行 `text` 光标按 `Up`;另在两逻辑行 `text` 光标在第 2 行按 `Up`、在第 1 行再按 `Up`
- **THEN** 单行(首行==末行)`Up` 直接翻历史;两行时第 2 行 `Up` 先上移到第 1 行(不翻历史),第 1 行再 `Up` 才翻历史(存多行草稿)

#### Scenario: 框高封顶、保底与极小终端(纯逻辑)

- **WHEN** 给定屏高与远超封顶的文本行数;另给定极小屏高
- **THEN** input 内容高 = `cap`(不超绝对上限、正常屏保 transcript ≥ 保底);极小屏时 `cap` 仍 **≥ 1**(不为 0),超出显示行经框内滚动到光标 visual 行可见

#### Scenario: 多行渲染 + 光标 + 软换行定位(insta 快照)

- **WHEN** 构造含普通多行与一条**超宽逻辑行**的 `text` 与某 cursor(含落在软换行满宽边界的 case),`TestBackend` 渲染
- **THEN** 快照显多行 + 超宽行软换行、框高随行数增大、光标在 logical→visual 映射后的正确 `(显示行, 列)`(边界处归下一显示行首);transcript 相应缩但不低于保底

#### Scenario: 硬模态吞键、软浮层补全仍过滤

- **WHEN** `pending_permission` 或 `models_picker` 活跃时收到换行/编辑/光标键;另在 `command_completion` 活跃时键入一个 `Char`
- **THEN** 硬模态下这些键归模态处理、MUST NOT 改文本缓冲;`command_completion` 下 `Char` **仍改缓冲并重过滤候选**(不被当作「浮层活跃即禁编辑」)

## MODIFIED Requirements

### Requirement: 输入历史 ↑↓ 召回(本会话内存)

系统 SHALL 在**主输入态**(无浮层:无 pending 权限框、无 `/` 命令补全、无 models picker)下,把 `↑` / `↓` 经**合并动作 `Up` / `Down`** 按**多行光标优先、边界翻历史**分流(由文本缓冲 reducer 按 cursor 逻辑行位置分派):光标**不在首逻辑行**时 `Up` SHALL 上移一行、**不在末逻辑行**时 `Down` SHALL 下移一行(按 `display` 列对齐);光标在**首逻辑行** `Up` / **末逻辑行** `Down` SHALL 召回上/下一条**已提交输入**(普通 prompt 与 `/命令` 均入历史)。进入历史前的草稿(可为多行)MUST 被保存,游标越过最新一条时 `Down` SHALL 恢复该草稿;召回一条历史后 cursor MUST 置于文本末尾;在历史某条上**键入字符**或**提交** MUST 重置游标回草稿态。连续两次提交相同文本 MUST 只入历史一条。历史仅存于本会话内存,关闭 TUI 即清空(不落盘)。历史/光标导航为纯函数 reducer,可单测。命令补全 / picker 打开时 `↑↓` 归各自浮层处理(优先级最高,在投 `Up`/`Down` 之前拦截),历史与光标移动均不参与。

#### Scenario: 单行时 Up 逐条回溯、Down 前进

- **WHEN** 依次提交 `a`、`b`,在**单行**空输入态按 `Up`
- **THEN** 输入框为 `b`(首行==末行,`Up` 直接翻历史);再按 `Up` → `a`;在 `a` 上按 `Down` → 回到 `b`

#### Scenario: 多行时 Up/Down 先移光标、边界才翻历史

- **WHEN** 输入框含两逻辑行文本,光标在**第 2 行**按 `Up`
- **THEN** 光标上移到第 1 行(不翻历史);此时再按 `Up`(已在首行)→ 才召回上一条历史(存当前多行草稿);在末行按 `Down` 越过最新 → 恢复该多行草稿

#### Scenario: Down 越过最新恢复草稿

- **WHEN** 输入框已键入未提交草稿 `dr`,光标在末行按 `Up` 进入历史,再按 `Down` 越过最新一条
- **THEN** 输入框恢复为 `dr`

#### Scenario: 键入字符脱离历史

- **WHEN** 处于历史某条,键入一个字符
- **THEN** 游标回草稿态,该字符插入到光标处,后续首行 `Up` 从最新条重新回溯

#### Scenario: 连续重复提交去重

- **WHEN** 连续两次提交相同文本 `x`
- **THEN** 历史中只保留一条 `x`

#### Scenario: 浮层打开时 ↑↓ 不归历史也不移光标

- **WHEN** `/` 命令补全浮层打开,按 `↑↓`
- **THEN** 由补全浮层处理高亮移动,输入历史与文本光标均不变

### Requirement: transcript 滚动

`AppState` SHALL 维护 transcript 的 `scroll_offset`:默认**跟随底部**(新内容自动到底);手动滚动支持 **PageUp / PageDown**(整页)、**`Ctrl+Home`(`scroll_to_top`,到顶)**、**`Ctrl+End`(`scroll_to_bottom`,回底并恢复底部跟随)**;**鼠标滚轮**(`MouseEventKind::ScrollUp` / `ScrollDown`)经行级步进滚动(默认每次 N 行)。**`↑` / `↓` 与裸 `Home` / `End` 不再用于 transcript 滚动**——`↑↓` 归多行光标 / 输入历史(见「输入历史 ↑↓ 召回」)、裸 `Home` / `End` 归输入行内光标(见「多行输入编辑」)。`scroll_to_top` MUST 置 `scroll_offset = 0` 且 `follows_bottom = false`;`scroll_to_bottom` MUST 置 `follows_bottom = true`(下一帧贴底)。滚到非底部时新内容 MUST NOT 强制拉回底部;滚回底部时 MUST 恢复跟随;offset MUST clamp 在 [顶, 底]。**仅 transcript 滚动**,顶栏 / 状态行 / 输入框 / 权限框固定。所有滚动键处理 SHALL 仅响应 `KeyEventKind::Press`。鼠标滚轮要求终端 guard 进入时启用、退出 / panic 时关闭鼠标捕获。offset / 跟随逻辑 MUST 可单测。

#### Scenario: 跟随、手动滚、clamp(逻辑可测)

- **WHEN** 在底部时追加新内容 → 仍贴底;PageUp 后追加新内容 → 保持当前位置;PageUp/PageDown 至边界 → offset clamp 不越顶 / 底
- **THEN** `scroll_offset` 按上述规则变化(纯逻辑断言)

#### Scenario: 行级 / 鼠标滚轮步进与触底恢复跟随(逻辑可测)

- **WHEN** 调 `scroll_up`(行级)上滚若干行,再 `scroll_down` 步进直至触底
- **THEN** 上滚后 `follows_bottom` 为假且 offset 按行级步进变化;触底后 `follows_bottom` 恢复为真

#### Scenario: Ctrl+Home 到顶 / Ctrl+End 回底,裸 Home/End 不滚动(逻辑可测)

- **WHEN** 在跟随底部态调 `scroll_to_top`(`Ctrl+Home`),随后调 `scroll_to_bottom`(`Ctrl+End`)
- **THEN** `scroll_to_top` 后 `scroll_offset == 0` 且 `follows_bottom == false`;`scroll_to_bottom` 后 `follows_bottom == true`、`visible_scroll_offset` 回到底部偏移;裸 `Home`/`End` 不改 `scroll_offset`

#### Scenario: 滚动后的 transcript 快照

- **WHEN** transcript 行数超视口且 `scroll_offset` 指向中段时渲染
- **THEN** 快照只显对应窗口的 transcript 行,顶栏 / 状态行 / 输入框位置不变

### Requirement: 键盘滚动全覆盖与鼠标滚轮降级(ConPTY 限制)

键盘 SHALL 提供 transcript 的导航且**不依赖**鼠标捕获:整页(`PageUp` / `PageDown`)、到顶(`Ctrl+Home`)、回底并恢复跟随(`Ctrl+End`)合起来 MUST 能从任意位置到达 transcript 的**顶**与**底**。**`↑` / `↓` 不用于 transcript 滚动**——在主输入态改为多行光标 / 输入历史导航(见「输入历史 ↑↓ 召回」);**裸 `Home` / `End` 不用于 transcript 滚动**——改为输入行内光标(见「多行输入编辑」)。故纯键盘滚动以**页级 + 边界**(`PageUp` / `PageDown` / `Ctrl+Home` / `Ctrl+End`)覆盖到顶 / 底;**不再保证逐行键盘滚动**,逐行仅在转发滚轮的终端经鼠标滚轮提供(ConPTY 无滚轮时只能页级遍历——此为在 ↑↓ / Home-End 抢键冲突上的取舍)。鼠标滚轮(`MouseEventKind::ScrollUp` / `ScrollDown`)SHALL 作为**尽力而为**的增强:在转发滚轮事件的终端可用;在 **ConPTY 不转发滚轮的 Windows 构建**上滚轮事件不到达 crossterm,此失效 MUST NOT 削弱键盘的页级 + 边界覆盖(滚轮缺失时仍可纯键盘到达顶 / 底)。`scroll_up` / `scroll_down` 原语 MUST 保留供页级实现与滚轮复用。`terminal.rs` 的鼠标捕获 MUST 保持开启(失效根因在平台而非捕获缺失,不因本 change 关闭捕获)。

#### Scenario: 纯键盘到顶与回底(无任何 MouseEvent)

- **WHEN** transcript 行数超视口、跟随底部态,**不**投入任何 `Event::Mouse`,仅以键盘调 `scroll_to_top`(`Ctrl+Home`)再 `scroll_to_bottom`(`Ctrl+End`)
- **THEN** `scroll_to_top` 后 `visible_scroll_offset` 指向顶(0)、`follows_bottom` 为假;`scroll_to_bottom` 后 `follows_bottom` 为真且回到底部偏移

#### Scenario: ↑ / ↓ 与裸 Home / End 不再滚 transcript

- **WHEN** 主输入态(无浮层)按 `↑` 或裸 `Home`
- **THEN** transcript 滚动位置不变;`↑` 归多行光标/输入历史、裸 `Home` 归输入行首光标
- **WHEN** 需要键盘滚 transcript 到顶
- **THEN** 用 `Ctrl+Home`(到顶)或 `PageUp`(页级),`↑` / `↓` / 裸 `Home` / `End` 不参与滚动

### Requirement: 跳到底部提示与新消息计数

当 transcript **未跟随底部**(用户经 `PageUp` / `Ctrl+Home` 等滚离底部)时,系统 SHALL 在 transcript 视口底部、输入框上方钉一条单行提示 pill;**跟随底部时 pill MUST 隐藏**。pill 文案两态:自滚离底部以来无新增助手消息时为 `跳到底部 (ctrl+End) ↓`;新增了 N(≥1)条**已完成的助手消息**时为 `N 条新消息 (ctrl+End) ↓`。新消息计数 SHALL 只计助手消息(一轮答复 = 1),MUST NOT 计 user 回显 / 工具卡 / notice;计数在**未跟随底部**期间累加、回到底部跟随时 MUST 清零。`Ctrl+End` SHALL 使 transcript 回底并恢复跟随(**裸 `End` 归输入行内光标、不再回底**),回底后 pill 隐藏、计数清零。pill 渲染 SHALL 局部覆盖(仅 pill 宽,不留全宽黑带),配色用 theme token(adapt 设计规范 C14),含 `↓` glyph。计数增量逻辑 SHALL 为纯函数、可单测。

#### Scenario: 跟随底部时无 pill

- **WHEN** transcript 处于跟随底部态
- **THEN** 不渲染跳到底部 pill,新消息计数为 0

#### Scenario: 滚离底部显示「跳到底部」

- **WHEN** 用户经 `PageUp` 滚离底部,其间无新增助手消息
- **THEN** 视口底部渲染 `跳到底部 (ctrl+End) ↓`

#### Scenario: 滚离底部期间新助手消息累加(仅助手)

- **WHEN** 已滚离底部,模型完成 1 条助手回复(其间含若干工具卡)
- **THEN** pill 显示 `1 条新消息 (ctrl+End) ↓`(工具卡 / user 回显不计)
- **WHEN** 再完成 1 条助手回复
- **THEN** pill 显示 `2 条新消息 (ctrl+End) ↓`

#### Scenario: Ctrl+End 回底清零

- **WHEN** pill 显示 `2 条新消息 (ctrl+End) ↓`,按 `Ctrl+End`
- **THEN** transcript 回底并恢复跟随,pill 隐藏,计数清零

### Requirement: 鼠标滚轮滚动(捕获鼠标)

TUI SHALL 启用鼠标捕获:`TerminalGuard` 进入 alternate screen 时发 `EnableMouseCapture`,退出 / panic 时经 `restore_terminal` 发 `DisableMouseCapture`,使鼠标滚轮以 `Event::Mouse` 到达程序而非被终端翻译为 `↑/↓` 方向键。`MouseEventKind::ScrollUp` / `ScrollDown` SHALL 经 `scroll_up` / `scroll_down` 原语驱动 transcript 上 / 下滚动(每事件固定行数),**滚动时若存在选区 MUST 清除选区**(见「鼠标拖选与复制」)。`Down(Left)` / `Drag(Left)` / `Up(Left)` 用于 app 自管拖选复制(见「鼠标拖选与复制」);**除滚轮与上述选区用 kind 外的 mouse kind MUST 被忽略(不改交互)**。键盘 `↑↓` MUST NOT 受影响(仍归多行光标 / 输入历史)。鼠标捕获 MUST 在退出 TUI / panic 时正确解除(沿用 `restore_terminal` 单一路径,不残留鼠标模式)。**终端原生框选让位于 app 自管拖选复制(无需 Shift)**。

**降级**:部分 Windows ConPTY 构建即便捕获也可能不转发滚轮事件——此时滚轮无效,但 MUST NOT 影响键盘滚动(`PageUp` / `PageDown` / `Ctrl+Home` / `Ctrl+End`)与 `↑↓` 历史/光标(键盘全覆盖不受损)。

#### Scenario: 进入 TUI 启用鼠标捕获、退出解除

- **WHEN** 构造 `TerminalGuard` 进入 alternate screen
- **THEN** 终端被置入鼠标捕获模式(setup 发 `EnableMouseCapture`)
- **WHEN** TUI 退出或 panic,经 `restore_terminal`
- **THEN** `DisableMouseCapture` 被发出,终端恢复原生鼠标行为(无残留)

#### Scenario: 滚轮事件驱动 transcript 滚动并清除选区

- **WHEN** 收到 `Event::Mouse` 且 kind 为 `ScrollUp`
- **THEN** 经 `scroll_up` 原语上滚 transcript(固定行数),若有选区则清除
- **WHEN** 收到 `Event::Mouse` 且 kind 为 `ScrollDown`
- **THEN** 经 `scroll_down` 原语下滚 transcript,若有选区则清除

#### Scenario: 键盘 ↑↓ 不受滚轮捕获影响

- **WHEN** 鼠标捕获已启用,主输入态按键盘 `↑`
- **THEN** 仍归多行光标 / 输入历史(滚轮捕获不改键盘 `↑↓` 语义)

### Requirement: 鼠标拖选与复制(捕获鼠标下 app 自管选区)

在已捕获鼠标的全屏 alt-screen 下,TUI SHALL 由程序自管一段**线性选区**,使拖选复制与滚轮滚动、`↑↓` 共存且**无需按 Shift**。选区状态 MUST 为纯逻辑(与 ratatui 解耦)、可单测:`SelectionState { selection: Option<Selection>, dragging }`,`Selection { anchor: Point, head: Point }`,`Point { col, row }`(屏幕 cell 坐标)。`reduce_selection(state, action)` 归约:`Press(p)` 置锚点并起拖(`anchor=head=p, dragging=true`,若已有旧选区则被新拖选覆盖);`Drag(p)` 在拖拽中扩选(`head=p`);`Release(p)` 结束拖拽,**无位移(`anchor==head`,即单击)MUST 清除选区**、有位移 MUST 保留定稿选区;`Clear` 清空。归约 MUST NOT 产生副作用(复制由调用方按「归约后是否仍有选区」触发)。

事件循环 MUST 把 `MouseEventKind::Down(Left)/Drag(Left)/Up(Left)` 连同 `column/row` 映射为 `Press/Drag/Release`(现丢弃坐标的写法 MUST 改为透传坐标);`Up(Left)` 归约后仍有选区时 MUST 触发复制。**松开即复制且选区高亮保留**(不因复制而清除,与终端原生选区一致);**`Ctrl+C` 在有选区时亦复制并保留选区**(优先级见 MODIFIED「运行中可中断」:`pending_permission` 存在时不复制、维持原行为)。**复制本身 MUST NOT 清除选区**;清除选区的触发为:**新拖选(`Press`)/ 任意滚动(滚轮 `ScrollUp/Down` 与键盘 `PageUp/PageDown/Ctrl+Home/Ctrl+End`)/ 窗口 `Resize` / 提交输入(Enter)/ `/clear` / 单击未拖动 / `Esc`(有选区时)**。**裸 `Home`/`End` 为输入行内光标移动(见「多行输入编辑」),MUST NOT 清除选区**(它不滚动 transcript)。

选区文本 MUST 从**刚渲染的 buffer**(`terminal.draw` 返回的 `CompletedFrame.buffer`,非 `current_buffer_mut()`)读取,不得由 transcript + 布局反推。纯函数 `selection_text(buffer, sel) -> String` MUST:按 `col_range_for_row` 求每行列区间(单行 `start.col..=end.col`;跨行首行至行尾、中间行整行、末行至 `end.col`)、逐 cell 读 `symbol()`、**按 symbol 的显示宽度推进游标**——读到宽字符(display width = 2)后 positionally 跳过其后 `width-1` 个延续格(**延续格 symbol 实为单空格 `" "` 而非空串,MUST NOT 依赖 `is_empty()` 判定**;此即 ratatui 自身 diff / Buffer Debug 的做法)、每行 `trim_end`、行间 `\n` join。全屏下 `MouseEvent.column/row` 即 buffer cell 坐标(左上 `0,0`),直接映射。

选区高亮 MUST 由 `render` 末尾一趟 overlay pass 实现:遍历选区覆盖的 cell,把对应 cell 的 `bg` 置为新 token `selection.bg`(仅改背景、保留前景字形)。`Theme` MUST 加 `selection_bg` 字段,`midnight()` / `daylight()` 双调色板各给值,`tokens()` 计入,`theme.rs` 单测锁定二值。

**防 panic**:取文与高亮读写 cell MUST 用 `Buffer::cell` / `cell_mut`(返回 `Option`)或先把 `Point` clamp 到 `buffer.area`,MUST NOT 用裸 `Index`(越界会 panic);`MouseEvent.column/row` 由终端上报、不保证在界内,MUST 视为不可信坐标。复制经 `trait Clipboard { fn set_text(&mut self, text) -> Result<(), String> }` 注入,真实现包 `arboard`;复制失败(初始化 / 写入 `Err`)MUST **静默降级**为 `AgentEvent::Notice`(**不清除选区**),MUST NOT panic 或阻塞主循环。选区文本 `trim` 后为空(纯空白 / 空选区)或 `last_frame` 尚为 `None` 时,MUST **跳过复制**、不触碰系统剪贴板。v1 选区 MUST 限**当前可见视口**内(不做拖动自动滚 / 不选已滚出内容)。浮层(models_picker / 命令补全 / 权限框)打开时拖选 **按所见 cell 取文(WYSIWYG)**,不为浮层设特例(v1 不禁用拖选)。

#### Scenario: 选区归约起选 / 扩选 / 单击清除(纯逻辑)

- **WHEN** 对 `SelectionState::default()` 依次归约 `Press(2,1)` → `Drag(6,1)` → `Release(6,1)`
- **THEN** `Press` 后 `selection=Some{anchor:(2,1),head:(2,1)}, dragging=true`;`Drag` 后 `head=(6,1)`;`Release` 后 `dragging=false` 且选区**保留**;而 `Press(3,0)` → `Release(3,0)`(无位移单击)后 `selection=None`

#### Scenario: 选区规范化与逐行列区间(纯逻辑)

- **WHEN** `Selection{anchor:(6,2), head:(2,0)}` 求规范化及各行列区间(视口宽 W)
- **THEN** 规范化为 `start=(2,0), end=(6,2)`(reading order,start≤end);首行(row 0)列区间为 `2..W`、中间行(row 1)为 `0..W`、末行(row 2)为 `0..=6`;同行选区 `start.row==end.row` 时区间为 `start.col..=end.col`

#### Scenario: 从 buffer 取选区文本按显示宽度跳延续格(纯函数)

- **WHEN** 用 `Buffer::set_string` 真实写入含 CJK 宽字符的行(如「你好」占 4 列 + 尾随补白,延续格 symbol 为 `" "`),对覆盖该行的选区调 `selection_text`
- **THEN** 返回「你好」原文(读到宽字符后跳过其后 1 个延续格,**两个 CJK 间无多余空格**)、行尾补白被 `trim_end`、跨行以 `\n` 连接

#### Scenario: 选区高亮叠加背景色且松开后保留(insta 快照)

- **WHEN** 给定一段定稿(已 `Release`)选区,经 `TestBackend` 渲染 transcript 并叠加高亮
- **THEN** 带色快照中选区覆盖的 cell 背景为 `selection.bg`、前景字形不变,松开后高亮仍在,与锁定快照一致

#### Scenario: selection.bg token 双调色板锁值(theme.rs 单测)

- **WHEN** 读取 `Theme::midnight()` 与 `Theme::daylight()` 的 `selection_bg`
- **THEN** 二者为设计规范约定的固定值,`tokens()` 含 `("selection.bg", _)`,单测锁定

#### Scenario: 松开复制与 Ctrl+C 复制写入剪贴板且保留选区(注入 Clipboard)

- **WHEN** 以 mock `Clipboard` 注入(无 pending),拖出一段选区后 `Up(Left)`;另在有选区时按 `Ctrl+C`
- **THEN** 两种路径都调用 `Clipboard::set_text` 传入 `selection_text` 结果、**复制后选区与高亮保留**(不清除)、`Ctrl+C` 有选区时**不触发退出**;`pending_permission` 存在时 `Ctrl+C` 不复制(维持原行为)

#### Scenario: 复制失败静默降级为 Notice(注入失败 Clipboard)

- **WHEN** mock `Clipboard::set_text` 返回 `Err`,触发一次复制
- **THEN** transcript 末尾出现一条复制失败 `Notice`、不 panic、主循环继续、**选区保留**(不因复制失败而清)

#### Scenario: 空 / 纯空白选区跳过复制不触剪贴板

- **WHEN** 选区仅覆盖空白 cell(`selection_text` 经 `trim` 后为空),或 `last_frame` 尚为 `None` 时触发复制
- **THEN** 不调用 `Clipboard::set_text`、不触碰系统剪贴板、不 panic

#### Scenario: 滚动 / resize 清除选区,裸 Home/End 不清

- **WHEN** 存在定稿选区时收到 滚轮 `ScrollUp/Down`、或键盘 `PageUp/PageDown/Ctrl+Home/Ctrl+End`、或 `Event::Resize`;另在有选区时按裸 `Home`/`End`
- **THEN** 前者任一清除选区(`selection=None`,避免高亮 / 取文指向滚动或缩放后的错误内容);裸 `Home`/`End`(输入行内光标)**不清除选区**

#### Scenario: 越界坐标不 panic(纯函数 / 防御)

- **WHEN** 选区的 `Point` 超出 buffer 边界(终端上报越界坐标 / resize 失配),对其调 `selection_text` 或高亮 pass
- **THEN** 经 `Buffer::cell`/`cell_mut` 的 `Option` 短路或 clamp 安全处理,不 panic

### Requirement: 终端文本排版与宽度度量

`render` SHALL 提供按显示宽度的 transcript 文本排版:① `User` / `Assistant` 文本块按视口宽度**换行**,续行**悬挂缩进**对齐 marker 宽度;② `Assistant` marker 为 `◆ `、`User` marker 为 `> `;③ 宽度度量 `display_width` MUST 把 CJK / 全角与常见 emoji 记为 2 列、组合 / 零宽字符记为 0 列(`display_width` / `char_width` 置于中立 `tui/width.rs`,供 render 与文本缓冲 reducer 共用);④ 输入框 MUST 把终端光标定位到 **cursor 所在 `(逻辑行, display 列)`** 经 logical→visual 映射后的位置(**多行编辑下光标不再恒在输入串末尾**);⑤ C2 欢迎态(`设计规范/03`)文本**水平居中**,空会话时整体**垂直留白**居中。以上 MUST 可经 `TestBackend` 渲染断言 / `insta` 快照验证。

#### Scenario: 显示宽度度量

- **WHEN** 计算 `display_width("a")` / `display_width("你好")` / `display_width("👋")`
- **THEN** 分别为 `1` / `4` / `2`

#### Scenario: Assistant 块换行 + 悬挂缩进 + 宽度

- **WHEN** 一个超视口宽度、含 emoji 的 `Assistant` 块渲染到窄 `TestBackend`
- **THEN** 首行以 `◆ ` 起、续行按 marker 宽度悬挂缩进对齐,文本按显示宽度换行(不串列、不撑破视口)

#### Scenario: 输入框光标定位到 cursor 位置

- **WHEN** 单行输入 `你好`(cursor 在末尾)时渲染;另在多行输入、cursor 在**第 1 逻辑行行首**时渲染
- **THEN** 前者光标位于 `prompt + 你好显示宽度` 之后的列;后者光标位于第 1 显示行、prompt 之后的行首列(不再恒定位到输入串末尾)

#### Scenario: 欢迎态居中(快照)

- **WHEN** 空会话渲染到 `TestBackend`
- **THEN** 带色快照中 C2 欢迎态文本水平居中、整体垂直留白居中,与锁定快照一致
