## ADDED Requirements

### Requirement: 鼠标拖选与复制(捕获鼠标下 app 自管选区)

在已捕获鼠标的全屏 alt-screen 下,TUI SHALL 由程序自管一段**线性选区**,使拖选复制与滚轮滚动、`↑↓` 输入历史三者共存且**无需按 Shift**。选区状态 MUST 为纯逻辑(与 ratatui 解耦)、可单测:`SelectionState { selection: Option<Selection>, dragging }`,`Selection { anchor: Point, head: Point }`,`Point { col, row }`(屏幕 cell 坐标)。`reduce_selection(state, action)` 归约:`Press(p)` 置锚点并起拖(`anchor=head=p, dragging=true`,若已有旧选区则被新拖选覆盖);`Drag(p)` 在拖拽中扩选(`head=p`);`Release(p)` 结束拖拽,**无位移(`anchor==head`,即单击)MUST 清除选区**、有位移 MUST 保留定稿选区;`Clear` 清空。归约 MUST NOT 产生副作用(复制由调用方按「归约后是否仍有选区」触发)。

事件循环 MUST 把 `MouseEventKind::Down(Left)/Drag(Left)/Up(Left)` 连同 `column/row` 映射为 `Press/Drag/Release`(现丢弃坐标的写法 MUST 改为透传坐标);`Up(Left)` 归约后仍有选区时 MUST 触发复制。**松开即复制且选区高亮保留**(不因复制而清除,与终端原生选区一致);**`Ctrl+C` 在有选区时亦复制并保留选区**(优先级见 MODIFIED「运行中可中断」:`pending_permission` 存在时不复制、维持原行为)。**复制本身 MUST NOT 清除选区**;清除选区的触发为:**新拖选(`Press`)/ 任意滚动(滚轮 `ScrollUp/Down` 与键盘 `PageUp/PageDown/Home/End/Ctrl+End`)/ 窗口 `Resize` / 提交输入(Enter)/ `/clear` / 单击未拖动 / `Esc`(有选区时)**。

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

#### Scenario: 滚动 / resize 清除选区

- **WHEN** 存在定稿选区时收到 滚轮 `ScrollUp/Down`、或键盘 `PageUp/PageDown/Home/End/Ctrl+End`、或 `Event::Resize`
- **THEN** 任一都清除选区(`selection=None`),避免高亮 / 取文指向滚动或缩放后的错误内容

#### Scenario: 越界坐标不 panic(纯函数 / 防御)

- **WHEN** 选区的 `Point` 超出 buffer 边界(终端上报越界坐标 / resize 失配),对其调 `selection_text` 或高亮 pass
- **THEN** 经 `Buffer::cell`/`cell_mut` 的 `Option` 短路或 clamp 安全处理,不 panic

## MODIFIED Requirements

### Requirement: 运行中可中断(Esc 中断本轮)

`tui` 的 `UserInput` SHALL 增 `Interrupt` 变体、`AgentEvent` SHALL 增 `Interrupted` 变体。`run_agent_task` 的 `Prompt` 分支 MUST 以 `tokio::select!` 把本轮 `Agent::run_observed` 与一个**独立**中断信号并置:中断信号 MUST NOT 复用 `input_rx`(以免误吞排队的 `Prompt` / `SetModel`)。中断到达即 drop 本轮 run future(在 `provider.complete` / `tool.execute` 的 await 点协作取消),向 UI 发 `AgentEvent::Interrupted`、状态回 `Idle`,**agent task 与程序均存活**;中断后 MUST NOT 再次调用 provider。UI 端 Esc 按**分流**(仅响应 `KeyEventKind::Press`,**模态优先于选区**):`pending_permission` 存在 → 拒绝授权(**最高优先**,不变);否则存在选区 → 清除选区(消费,不退出不中断);否则本轮运行中(phase 非 Idle/Ready)→ 投 `UserInput::Interrupt`(中断);否则就绪 → 退出程序(不变)。`Ctrl+C` 分流同序:`pending_permission` 存在 → 维持原行为(不复制、不退出);否则存在选区 → 复制并保留选区(不退出);否则 → 退出。

#### Scenario: 运行中中断以 Interrupted 收场且不再调用 provider

- **WHEN** 以 Mock provider(在 `complete` 中挂起的脚本)驱动 `run_agent_task`,投入 `Prompt` 后再投 `Interrupt`
- **THEN** 本轮以 `AgentEvent::Interrupted` 收场、状态回 `Idle`,provider 不被再次调用,agent task 继续存活待下一个 `UserInput`(全程无终端)

#### Scenario: 中断不消费排队的 Prompt

- **WHEN** 中断信号经独立通道到达,而 `input_rx` 中另有排队的 `Prompt`
- **THEN** 仅本轮被中断;排队的 `Prompt` 不被中断臂吞掉,后续仍可正常消费并跑完

#### Scenario: Esc 分流(模态优先于选区)

- **WHEN** 分别在「pending 授权 + 有选区」/「有选区、无 pending」/「本轮运行中、无 pending 无选区」/「就绪、无 pending 无选区」下收到 Esc(`KeyEventKind::Press`)
- **THEN** 依次为:经 oneshot 回送 `Deny` 拒绝授权(**pending 优先,不先清选区**)/ 清除选区(消费、不退出不中断)/ 投出 `UserInput::Interrupt` / `should_exit` 为真(退出);优先级 pending > 选区 > 运行中中断 > 就绪退出

#### Scenario: 中断态渲染为非致命 notice

- **WHEN** UI 收到 `AgentEvent::Interrupted` 后渲染
- **THEN** transcript 末尾含一条「⊘ 已中断本轮」notice 块(`info.fg` / 非致命,区别于 C7 致命错误框),与锁定带色快照一致

### Requirement: 鼠标滚轮滚动(捕获鼠标)

TUI SHALL 启用鼠标捕获:`TerminalGuard` 进入 alternate screen 时发 `EnableMouseCapture`,退出 / panic 时经 `restore_terminal` 发 `DisableMouseCapture`,使鼠标滚轮以 `Event::Mouse` 到达程序而非被终端翻译为 `↑/↓` 方向键。`MouseEventKind::ScrollUp` / `ScrollDown` SHALL 经 `scroll_up` / `scroll_down` 原语驱动 transcript 上 / 下滚动(每事件固定行数),**滚动时若存在选区 MUST 清除选区**(见「鼠标拖选与复制」)。`Down(Left)` / `Drag(Left)` / `Up(Left)` 用于 app 自管拖选复制(见「鼠标拖选与复制」);**除滚轮与上述选区用 kind 外的 mouse kind MUST 被忽略(不改交互)**。键盘 `↑↓` MUST NOT 受影响(仍归输入历史)。鼠标捕获 MUST 在退出 TUI / panic 时正确解除(沿用 `restore_terminal` 单一路径,不残留鼠标模式)。**终端原生框选让位于 app 自管拖选复制(无需 Shift)**。

**降级**:部分 Windows ConPTY 构建即便捕获也可能不转发滚轮事件——此时滚轮无效,但 MUST NOT 影响键盘滚动(`PageUp` / `PageDown` / `Home` / `End`)与 `↑↓` 历史(键盘全覆盖不受损)。

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
- **THEN** 仍召回输入历史(滚轮捕获不改键盘 `↑↓` 语义)
