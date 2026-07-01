## Context

当前 TUI 是**全屏 alt-screen**(`terminal.rs`:`EnterAlternateScreen + EnableMouseCapture`),已捕获鼠标。捕获让滚轮以 `Event::Mouse` 到达程序(`mod.rs::handle_mouse_wheel` 驱动 transcript 滚动),但也**接管了拖拽**——终端不再做原生框选,用户只能 Shift+拖选。用户要求:全屏下 **滚轮滚动 + 拖选复制(不按 Shift)+ `↑↓` 输入历史** 三者同时可用。

终端鼠标协议下,「收滚轮」与「终端原生拖选」互斥(收滚轮必须捕获,捕获即夺走终端框选)。`↑↓` 是纯键盘,恒不冲突。真正的矛盾双方是 **滚轮 ⟷ 拖选**,经**鼠标捕获**这一必经环节耦合。全屏下唯一让二者共存且不按 Shift 的解法:**程序自管选区与复制**——在已捕获的鼠标下,程序自己消费 `Down/Drag/Up`、自绘高亮、自写系统剪贴板(opencode 同款思路)。

约束:Rust + ratatui 0.29 + crossterm 0.28;不引入 Agent SDK;新依赖需说明理由(本 change 引入 `arboard`);行为逻辑走强制 TDD、TUI 渲染走 `insta` 事后快照;视觉以 `设计规范/` 为准。

## Goals / Non-Goals

**Goals:**
- 全屏基线(HEAD)上新增「app 鼠标拖选 + 复制」,与既有滚轮、`↑↓` 历史三者并存。
- 拖选**不需按 Shift**;**松开即复制且高亮保留**(像终端原生选区)、**`Ctrl+C` 有选区时亦复制**。
- 选区归约(起选 / 扩选 / 规范化 / 清除)为纯逻辑,可单测;高亮走 `insta` 快照;buffer→文本为纯函数,可单测。
- 复制失败(如 headless / arboard 初始化失败)**静默降级**为 Notice,不 panic、不阻塞主循环;越界坐标 / resize 不 panic。

**Non-Goals:**
- **不做内联渲染**(`inline-render-core` 已废弃并删除,保持全屏)。
- **不做拖动自动滚动**:v1 仅支持**当前可见视口内**的选区(「可见视口」指当前屏上已渲染 = 未滚出的内容,非「仅 rows[1] 空间区」);选中已滚出屏幕的内容留后续。
- 不做块选(列矩形选区)、不做多段选区;仅**线性(流式)单段**选择。
- 不改滚轮 / `↑↓` / 权限 / 浮层等既有语义(仅新增选区一层);浮层打开时拖选照 WYSIWYG 取所见 cell,不设特例。
- 不实现 OSC52(本机 Windows 不走 SSH,arboard 直写系统剪贴板更可靠;见 Decisions)。

## Decisions

### D1. 剪贴板用 `arboard`,不用 OSC52
- **选 arboard**:跨平台直写系统剪贴板(Win32 / NSPasteboard / X11),本机可靠、无需终端支持。
- **弃 OSC52**:依赖终端支持转义序列,Windows Terminal 上已知不稳(opencode #5046 / #11996);其唯一优势(走 SSH)本项目当前用不上。
- **代价 / 治理**:+1 直接依赖。**用户已批准**引入 arboard(松开复制 + Ctrl+C 复制方案)。传递依赖以 Windows 下的 `clipboard-win` / `windows-sys` 为主,体量在可接受范围;task 1.1 加依赖后以 `cargo tree` 复核实际足迹,若异常膨胀再回报。

### D2. 选区状态仿 `input_history` 模式:纯逻辑模块 + `AppState` 持字段
- 新模块 `src/tui/selection.rs`:`Point { col, row }`、`Selection { anchor, head }`、`SelectionState { selection: Option<Selection>, dragging: bool }`、`SelectionAction`、`reduce_selection(state, action) -> state`。全部**纯逻辑、无 ratatui 依赖、可单测**(镜像既有 `reduce_input_history`)。
- `AppState` 加 `pub selection: SelectionState` 字段 + 薄方法(`apply_selection_action` / `clear_selection` / `has_selection`)。`render` 读该字段画高亮。
- **为何不把鼠标处理塞进 `on_key`**:鼠标事件当前就在 `mod.rs` 事件循环层(非 `app.rs`),保持这一分层;`mod.rs` 调 `state.apply_selection_action(...)`,复制副作用(读 buffer + arboard)留在 `mod.rs`。
- **备选(弃)**:把选区做成 `mod.rs` 的局部变量。弃因 `render` 需读选区、`app.rs` 提交时需清选区,状态必须挂在 `AppState`。

### D3. 归约语义:松开定稿+复制+**保留**高亮 / 单击清除
`reduce_selection` 归约表:
- `Press(p)` → `{ selection: Some{anchor:p, head:p}, dragging: true }`(锚点落定,起新拖选,旧选区被覆盖)。
- `Drag(p)` → `dragging` 时 `head = p`;否则 no-op。
- `Release(p)` → `dragging` 置 false;若 `anchor == head`(全程无位移=单击)→ `selection = None`(清除);否则**保留定稿选区**。
- `Clear` → `{ None, false }`。
- **复制触发**:`mod.rs` 收 `Up(Left)` 归约后,若 `selection.is_some()`(即有位移)→ 触发复制。归约本身不产生副作用,复制判定由调用方按「归约后是否仍有选区」决定,保持 reducer 纯。
- **复制不清除选区**(与终端原生一致):松开复制后高亮**保留**,使 `Ctrl+C` 能再复制、用户能看清所选。**清除**选区仅由这些触发:新拖选(`Press`)/ 任意滚动(滚轮 `ScrollUp/Down` + 键盘 `PageUp/PageDown/Home/End/Ctrl+End`)/ 窗口 `Resize` / 提交(Enter)/ `/clear` / 单击未拖动 / `Esc`(有选区,模态优先见 D6)。

### D4. 取文从**刚渲染的 buffer** 读,按显示宽度跳延续格
- **决策**:不从 `transcript` + 布局反推选区文本(宽字符 / 换行处极易错,proposal Risk ①),而是直接读**已渲染 buffer** 的 cell symbol。
- **拿哪一帧**:用 `terminal.draw(...)` 的返回值 `CompletedFrame.buffer`(公开字段 `&Buffer`,即刚渲染那帧),在选区活跃时 `.clone()` 进事件循环的 `last_frame: Option<Buffer>`。**不**用 `terminal.current_buffer_mut()`——ratatui `draw` 内 `flush` 后会 `swap_buffers`(先 `reset()` 另一 buffer 再翻转 `current`),故 `draw` 返回后 `current_buffer_mut()` 指向的是**已被 `reset()` 清空的空白 back buffer**(不是上一帧、更不是刚渲染那帧);只有 `CompletedFrame.buffer`(`= buffers[1-current]`)是「刚渲染那帧」的稳定句柄。
- **坐标系**:全屏 alt-screen 下 `MouseEvent.column/row`(绝对终端坐标,0 基)== buffer cell 坐标(左上 0,0),直接映射,无偏移。终端上报坐标不保证在界内,视为不可信(见 D5 防 panic)。
- **纯函数** `selection_text(buffer, sel) -> String`(置于 `render.rs`,该文件已引 ratatui buffer 类型):
  - 用 `selection::col_range_for_row` 求每行列区间(单行:`start.col..=end.col`;跨行首行:`start.col..宽`;中间行:整行;末行:`0..=end.col`)。
  - 逐 cell 读 `symbol()`,**按其 unicode 显示宽度推进游标**:读到宽字符(width 2)后 positionally 跳过其后 `width-1` 个延续格。**注意延续格 symbol 在 ratatui 0.29 是单空格 `" "` 而非空串**(`set_stringn` 对延续格调 `Cell::reset()`,`reset()` 置 symbol 为 `" "`),故 **MUST NOT** 靠 `symbol().is_empty()` 跳格——必须按显示宽度位移(ratatui 自身 diff / Buffer Debug 即此法:`to_skip = symbol().width().saturating_sub(1)`)。
  - 每行 `trim_end` 去渲染补白;行间 `\n` join。

### D5. 高亮 = `render` 末尾一趟 overlay pass(防 panic)
- `render` 全部 widget 画完后(含浮层),追加 `highlight_selection(frame, state, theme)`:遍历选区覆盖的 cell,把对应 cell 的 `bg` 置为新 token `selection_bg`(只改背景,保留前景字形)。
- **cell 读写一律用 `Buffer::cell` / `cell_mut`(`Option` 版)或先 clamp 到 `buffer.area`,禁止裸 `Index`**——越界即 panic;选区坐标源自不可信的 `MouseEvent`,且 resize 后 buffer 缩小、旧坐标可能越界。
- **resize 清选区**:`Event::Resize` 纳入清除触发(D3),从根上避免旧屏幕坐标索引新尺寸 buffer;`Option`/clamp 是第二道防线。
- **新增 token `selection_bg`**:`Theme` 加字段 + `midnight()` / `daylight()` 各给值 + `tokens()` 数组 `19→20`;`theme.rs` 单测锁双调色板值。该 token 归属本 change 的新能力(纯增量,不改既有 token 枚举 requirement)。
- v1 选区范围限当前可见视口;高亮只作用于屏内 cell。

### D6. `Ctrl+C` / `Esc` 分流:模态 > 选区 > 中断/退出
- 事件循环处理 `Ctrl+C` / `Esc` 时,优先级 **`pending_permission`(模态)> 选区 > 运行中中断 / 就绪退出**:
  - `pending_permission` 存在:`Esc` → 拒授权(不变)、`Ctrl+C` → 维持原(HEAD 下 pending 时 `should_exit` 返回 false,即 no-op),**不因选区改变模态语义**。
  - 无 pending 但有选区:`Ctrl+C` → 复制并保留选区(不退出);`Esc` → 清除选区(消费,不退出不中断)。
  - 无 pending 无选区:维持原 `Esc` 三态(运行中中断 / 就绪退出)与 `Ctrl+C` 退出。
- **实现落点**:选区拦截须置于 pending / models_picker / 命令补全等既有守卫**之后**(它们由 `should_exit` 守卫 + `on_key_inner` 处理),不能像初版那样在 `should_exit` 之前无条件拦截(那会让选区凌驾模态——审查已判该为 bug)。
- 语义:选区是「浮起的临时态」,低于阻塞式授权模态、高于普通退出;`Ctrl+C`/`Esc` 先作用于选区,再按一次回到中断 / 退出。

### D7. `arboard` 经 `Clipboard` trait 注入 + 复制边界
- 定义 `trait Clipboard { fn set_text(&mut self, text: String) -> Result<(), String>; }`;真实现包 `arboard::Clipboard`,测试用 mock。
- **理由(非投机抽象)**:① 单测复制接线时不碰真实系统剪贴板(全局态、CI 易 flaky);② arboard 初始化 / 写入失败时统一降级路径。二者都是当前就需要的,不是「以防万一」。
- 事件循环启动时建一次 clipboard(失败则置降级态);`copy_selection` 用它,`Err` → 发 `AgentEvent::Notice("复制失败: …")`,**不 panic、不清除选区**。
- **复制边界**:`selection_text` 经 `trim` 后为空(纯空白 / 空选区),或 `last_frame` 尚为 `None`,MUST **跳过复制**(不触碰系统剪贴板,避免用一次误拖的空串覆盖用户剪贴板)。

## Risks / Trade-offs

- **[屏幕 cell ↔ 文本映射在宽字符 / 换行处易错]** → 直接读渲染 buffer 的 symbol(D4),不反推;宽字符按**显示宽度**位移跳延续格(**延续格是空格 `" "`,不能靠空串判定**);纯函数配 `Buffer::set_string` 真实构造 CJK 的单测锁定。
- **[arboard 在 headless / 缺显示环境初始化失败]** → `Clipboard` trait + 失败降级为 Notice(D7);主循环不受影响;注入失败 mock 单测覆盖降级。
- **[resize 后旧屏幕坐标索引更小 buffer → panic]** → resize 纳入清选区触发(D3/D5);cell 读写用 `Option`/clamp 作第二道防线;真机加「选中后缩窗」核验项。
- **[终端上报越界鼠标坐标 → 裸 Index panic]** → 坐标视为不可信,取文 / 高亮统一走 `Buffer::cell`/`cell_mut` 的 `Option` 短路或 clamp(D4/D5)。
- **[空 / 纯空白选区复制空串覆盖剪贴板]** → `trim` 后为空则跳过复制(D7)。
- **[选中已滚出视口的内容]** → v1 明确不做(Non-Goals);仅可见视口。拖到边缘不自动滚。后续可加。
- **[`Ctrl+C`/`Esc` 语义变更影响退出 / 模态]** → 模态优先、仅在无 pending 且有选区时改写,无选区严格维持原 `should_exit`;以 `should_exit`-风格纯逻辑测 + 归约测双向锁定,防误删退出、防选区凌驾模态。
- **[每帧克隆 buffer 的开销]** → 仅在选区活跃时克隆 `last_frame`;非选区态零额外开销。

## Open Questions

- 复制成功是否给轻提示(如短暂 Notice「已复制 N 字符」)?倾向**给**(与松开即复制的无感操作配一个可见反馈),但可 v1 先不做、真机体验后定。实现时以最小 Notice 起步,留待真机确认。
