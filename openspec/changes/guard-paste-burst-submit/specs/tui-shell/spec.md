## MODIFIED Requirements

### Requirement: 多行输入编辑(文本缓冲 + 光标 + 换行)

输入框 SHALL 支持**手动多行编辑**。核心为**纯逻辑文本缓冲**(与 ratatui 解耦、可单测):`text: String`(可含 `\n`)+ `cursor: usize`(字节位,**MUST 落在 char 边界**)+ 输入历史(list / index / draft)。归约 `reduce_*(state, action) -> state`,动作含 `InsertChar` / `InsertNewline` / `Backspace` / `Delete` / `MoveLeft` / `MoveRight` / `MoveLineStart` / `MoveLineEnd` / `Up` / `Down` / `SetText` / `PushSubmitted` / `InsertStr`(**`InsertStr` = 在光标处一次性插入整串正文,由 change `guard-paste-burst-submit` 加入,供批量 drain 合并粘贴正文、避免逐字符 O(n²) clone;插入后 cursor 到插入末尾、`history_cursor` 置 None,与 `InsertChar` 语义一致**)。行按 `\n` 切分;光标列以 `display_width` 计(宽字符感知)。`display_width` / `char_width` MUST 抽到中立模块(`tui/width.rs`)供缓冲 reducer 与 render 共用(纯逻辑不得反依赖 render 外壳)。

**换行 vs 提交**:`Ctrl+Enter`(`KeyCode::Enter` + `CONTROL`)/ `Shift+Enter`(`Enter` + `SHIFT`)/ `Ctrl+J`(`KeyCode::Char('j')` + `CONTROL`)SHALL 在光标处插入 `\n`;`Enter`(无 CONTROL/SHIFT)在**孤立敲击**(其所在事件批次里唯一的文本内容键)时 SHALL **提交**整段 `text`(trim 后为空则 no-op),在**粘贴突发批**(批内 ≥2 文本内容键)时 SHALL 作 `InsertNewline` 插入 `\n`、不提交(见「粘贴突发合并输入」requirement)。换行键判定 MUST 在通用 `KeyCode::Char(ch)` 插入分支**之前**;通用 `Char` 分支 MUST 过滤**带 CONTROL 且不带 ALT** 的字符(纯 `Ctrl+字符`,如 `Ctrl+J`,避免插入字面 `j`);**AltGr 合成字符(同时带 CONTROL+ALT)MUST 保留插入**(国际键盘)。设计 MUST NOT 依赖 kitty keyboard protocol(`PushKeyboardEnhancementFlags` 在 Windows 返 `Err`,会致 TUI 无法启动)、MUST NOT 改 `terminal.rs`。**命令解析仅单行**:提交时 `text` **含 `\n`** → 整段作 prompt(不解析命令);**不含 `\n`** 且以 `/` 起头 → 走 `parse_command`。

**光标导航**:`←` / `→` 按 char 边界步进(跨宽字符整步,不落字符中间);`Home` / `End`(无 CONTROL)到**当前行**首 / 尾;`Backspace` 删光标前一 char、`Delete` 删后一 char。`↑` / `↓` 由 app 投**合并动作** `Up` / `Down`,reducer 按 cursor **逻辑行**位置分派:cursor **不在首逻辑行**时 `Up` SHALL 上移一逻辑行、**不在末逻辑行**时 `Down` SHALL 下移一逻辑行(按 `display` 列对齐,落宽字符跨列中间取「**≤ 目标视觉列的最大 char 边界**」;不维护 goal-column);cursor 在**首逻辑行** `Up` / **末逻辑行** `Down` SHALL 翻输入历史(见「输入历史 ↑↓ 召回」)。**`Up`/`Down` 按逻辑行分派**:单条超宽逻辑行虽软换行成多显示行,其内部不逐显示行上下移(在该逻辑行即视为首/末行)——v1 限制。

**模态 vs 软浮层路由**:所有编辑/光标/换行键 MUST 落在既有守卫**之后**。**硬模态**(`pending_permission` / `models_picker`)活跃时,编辑/换行/光标键 MUST NOT 进入文本缓冲(归模态处理)。**软浮层 `command_completion`** 活跃时:`Char` / `Backspace` **仍 MUST 改文本缓冲并触发补全重过滤**(保持既有 `/` 补全「继续输入重新过滤」);`Up` / `Down` / `Tab` / `Enter` / `Esc` 归补全浮层;其余光标/换行键(`←→ Home End Delete` / 换行)在补全打开时 MUST NOT 破坏补全过滤(建议忽略或先关浮层再执行)。Tab 不插入 `\t`(仍归命令补全;无补全态 no-op)。

**渲染**:`render_input` SHALL 多行渲染 `text`;输入框内容高度 SHALL 随行数**动态、封顶**:`cap = clamp(屏高 - (顶栏3 + status_top_gap + permission_height + 活动1 + 状态1 + mode1 + input 边框2) - transcript_floor, 1, 绝对上限)`,`transcript_floor` 与 `rows[1] = Min(8)` 口径一致(=8);**cap MUST ≥ 1 内容行**(屏高极小时 input 保 1 行、必要时挤占 floor 而非算得 0)。超宽逻辑行 SHALL **软换行**;render MUST 做 **logical(逻辑行,列) → visual(显示行,列)** 映射,把终端光标 `set_cursor_position` 到 visual 坐标;**cursor 落逻辑行软换行边界(=某显示行满宽处)时 MUST 归入下一显示行行首**(唯一确定,供单测)。显示行 > cap 时框内滚动使**光标 visual 行**可见。缓冲归约、光标、宽字符列映射、命令仅单行判定、框高 cap、logical↔visual 映射 MUST 纯逻辑单测;多行渲染 + 光标 + 动态框高 + 软换行走 `insta` 快照。

#### Scenario: 换行键插入 `\n`、Enter 提交、命令仅单行(纯逻辑 + 接线)

- **WHEN** 光标在文本中,分别收到 `Enter+CONTROL`、`Enter+SHIFT`、`Char('j')+CONTROL`;另在含 `\n` 的多行 `text`(首行以 `/` 起)按**孤立**(其批次唯一文本键)的无 modifier `Enter`
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

#### Scenario: 粘贴正文合并为 InsertStr(纯逻辑)

- **WHEN** 对文本缓冲归约 `InsertStr(s)`(`s` 可含多字节 CJK)
- **THEN** `s` 在光标处一次性插入、cursor 到插入末尾且落 char 边界、`history_cursor` 置 None;与逐个 `InsertChar` 得到相同 `text`,但只 clone 一次

## ADDED Requirements

### Requirement: 粘贴突发合并输入(批量 drain 防误提交)

TUI 事件循环 SHALL 在每次有 crossterm 事件到达时,用**同步** `event::poll(Duration::ZERO)` + `event::read()` 把当前**已就绪**的事件抽干成一个有界 batch(不 await、不阻塞、与 `EventStream` 共用同一 internal reader 故不污染其 waker),整批处理完只渲染一次;并按"一批**文本内容键**(`Char` + 裸 `Enter`,Press-only)的规模"区分**粘贴突发**与**用户敲击**:突发(批内 ≥2 文本内容键)内的裸 `Enter` SHALL 作换行插入,仅当裸 `Enter` 是其批次里唯一文本内容键时才提交。硬模态在批处理中**逐键按当时活跃态**分治(非整批截断):`pending_permission` 活跃时首键应答后丢弃该批余下键;`models_picker` 活跃时每键透传给 picker(打字过滤/导航/选中 MUST NOT 丢失)。此机制 SHALL NOT 依赖 bracketed paste(Windows crossterm 不产 `Event::Paste`)、SHALL NOT 改 `terminal.rs`,复用既有文本缓冲的 `InsertNewline`/`InsertStr` 动作与既有 `on_key` 路由。**已知上限(Non-Goal)**:大 transcript 慢渲染下正常打字凑批(末字符+提交 Enter 落同批 → Enter 误判换行,再按一次即提交)、粘贴以换行结尾(末 Enter 归换行不自动提交)、粘贴含 Tab 丢失、模态关闭后同批粘贴尾 Enter 被丢弃——batch-size 启发式不解决,需真实到达时序或 bracketed paste。

#### Scenario: 粘贴多行整段进入缓冲、不逐行提交

- **WHEN** 一段多行文本被粘贴,产生一批(≥2 个文本内容键)瞬时到达的 `Char`/`Enter` 事件
- **THEN** 该批内所有裸 `Enter` 作为 `InsertNewline` 插入缓冲,连续 `Char` 正文合并为 `InsertStr` 插入缓冲
- **AND** 全程不触发提交,transcript 不新增 user 块,不向 agent 发出 prompt
- **AND** 整批处理完只渲染一次(不逐事件渲染)

#### Scenario: Release 事件不计入突发规模

- **WHEN** 用户手按一次 `Enter`,Windows 产生 `[Enter Press, Enter Release]` 两个事件落入同一 batch
- **THEN** 先 `is_key_press` 滤除 Release,批内文本内容键数 `n == 1`,该裸 `Enter` 判为**提交**(而非因 Release 使 n=2 被误判换行)

#### Scenario: 孤立回车仍然提交

- **WHEN** 用户手按一次 `Enter`(该批次里唯一的文本内容键,`modifiers=NONE`)
- **THEN** 走既有提交路径:trim 后非空则整段作为 prompt 提交、清空缓冲、入历史

#### Scenario: 前置守卫消费的键不计入突发规模

- **WHEN** 一批里含被前置守卫消费的键(如 `PageUp`)后紧跟一个裸 `Enter`
- **THEN** `PageUp` 归滚动、不计入文本内容键;`n == 1` → 该 `Enter` 判为**提交**

#### Scenario: 批内带 modifier 的换行键照常换行

- **WHEN** 一批事件里含 `Enter+CONTROL` / `Enter+SHIFT` / `Char('j')+CONTROL`
- **THEN** 这些键按既有 `on_key` 换行分支插入换行,不受"突发 vs 孤立"判定影响(该判定只接管裸 `Enter`)

#### Scenario: pending_permission 活跃时突发只应答首键

- **WHEN** `pending_permission` 活跃,且一批(≥2 键)突发到达、首键为裸 `Enter`
- **THEN** 首键经 `on_key` 命中权限分支正常应答(Allow),随即**丢弃该批余下键**(一串粘贴 `Enter` 不连答)、**不被降级为换行、不往隐藏缓冲插入杂散 `\n`**

#### Scenario: models_picker 活跃时突发过滤输入不丢失

- **WHEN** `models_picker` 活跃,且一批多个过滤 `Char`(如 "gpt")与导航/选中键同批到达
- **THEN** 每键透传给 `handle_models_picker_key`(`Char`→逐个 `push_filter_char`、`Up/Down`→导航、`Enter`→选中并关闭),**过滤字符全部生效、不被截断丢失**;不套用 burst 换行意图;picker 中途关闭后同批尾随的粘贴裸 `Enter` 被丢弃、不落缓冲/提交

#### Scenario: 软浮层补全不受突发护栏影响

- **WHEN** `command_completion` 浮层活跃,且一批 `Char`/`Backspace` 到达
- **THEN** 逐个改缓冲并重过滤候选(不触发硬模态截断),维持既有 `/` 补全行为

#### Scenario: 退出与中断在批处理中仍即时生效

- **WHEN** 一批事件中出现 `Ctrl+C`(无选区)或运行中的 `Esc`
- **THEN** 既有 `should_exit` / 中断守卫逐键前置生效,命中即退出或中断,不被同批余下事件延迟

#### Scenario: 不依赖 bracketed paste

- **WHEN** 运行于 Windows Terminal(crossterm 不产 `Event::Paste`)
- **THEN** 粘贴合并仅由 batch 突发启发式实现,`Event::Paste` 分支维持忽略,`terminal.rs` 不变
