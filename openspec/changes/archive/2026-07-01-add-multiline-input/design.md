## Context

输入框现状:`InputHistoryState { input: String, ... }`,仅末尾 `InsertChar`/`Backspace`,无光标移动,`Enter`=提交。`render_input` 单行渲染 + 定位光标到输入串末尾;`layout_rows` 给 input 固定 `Constraint::Length(3)`;`display_width`/`char_width` 是 `render.rs` 私有 fn。`Home`/`End` 现绑 transcript `scroll_to_top`/`scroll_to_bottom`(mod.rs `scroll_action_for_key`,忽略 modifier);`↑↓` 现绑输入历史。`terminal.rs` = alt-screen + `EnableMouseCapture`(已含选区功能)。

目标:输入框支持**手动多行编辑**——换行、光标编辑、Enter 提交整段。本设计经用户真机(Windows 11 + Windows Terminal)诊断日志校准换行键方案。

**范围**:本 change 只做「手动多行编辑」;**粘贴防狂发拆到 change B**(需事件循环批量 drain 避免 render 延迟污染 burst 判定,独立且启发式)。本 change **不改事件循环读取结构**、不做 burst;多行粘贴仍逐 Enter 提交(维持现状)。约束:Rust + crossterm 0.28 + ratatui 0.29;不引新依赖;纯逻辑强制 TDD、TUI 渲染事后快照;不破坏既有选区/滚轮/历史。

## Goals / Non-Goals

**Goals:**
- 多行文本缓冲 + 光标导航(`←→↑↓ Home End`),纯逻辑可单测。
- `Ctrl+Enter`/`Shift+Enter`/`Ctrl+J` 换行;`Enter` 提交整段(命令解析仅单行)。
- 多行渲染 + 输入框动态长高(封顶 + 内部滚动 + 软换行),transcript 留保底。
- `terminal.rs` 不改;不引新依赖;不破坏既有交互。

**Non-Goals:**
- **粘贴防狂发 / 突发检测 / 批量 drain**(→ change B);本 change 不做 `InsertStr` 批量插入、不测粘贴。
- **goal-column 记忆**(连续 `↑↓` 保持初始视觉列):v1 不做,每次 `↑↓` 以当前列对齐,连续跨不等长行可能列漂移(可接受)。
- **超宽软换行逻辑行的行内逐显示行上下移**:`Up`/`Down` 按逻辑行分派(不按显示行),单条被软换行的长逻辑行内部不支持视觉上下移(在该逻辑行首/末即翻历史 / 移邻逻辑行)。用 `←→` 在该行内移动。
- 输入区**鼠标框选**、词级跳转(`Ctrl+←→`)、查找替换、语法高亮、撤销重做。

## Decisions

### D1. 换行键用终端原生 modifier,不用 kitty;A 下 Enter 恒提交
- **实测依据**(用户 WT 诊断日志):`Ctrl+Enter`=`Enter+CONTROL`、`Shift+Enter`=`Enter+SHIFT`、`Ctrl+J`=`Char('j')+CONTROL`,原生带 modifier。
- **弃 `PushKeyboardEnhancementFlags`(kitty)**:Windows 上 `execute_winapi()` 返 `Err(Unsupported)`,并入 `TerminalGuard::new()` 的 `execute!(...)?` 会致 new() 返 Err、TUI 起不来。故不 push,`terminal.rs` 不改。
- **判定**:`Enter`+CONTROL 或 +SHIFT → `InsertNewline`;`Char('j')`+CONTROL(Ctrl+J,主键是字符不是 Enter)→ `InsertNewline`;`Enter`(无 CONTROL/SHIFT)→ **提交**(本 change 无 burst,Enter 恒提交)。
- **接线次序**(finding):换行键判定 MUST 在通用 `KeyCode::Char(ch)` 插入分支**之前**(`Ctrl+J` 匹配 `Char(c) if c.eq_ignore_ascii_case('j') && CONTROL`);通用 `Char` 分支插入前 MUST 过滤**带 CONTROL 且不带 ALT** 的字符(纯 `Ctrl+字符`,否则 Ctrl+J 会插字面 `j`);**AltGr 合成字符(同时带 CONTROL+ALT)保留插入**(国际键盘,别误吞)。

### D2. 文本缓冲 + 光标 纯逻辑(TDD),不含粘贴批量插入
- 新模型(重构 `input_history` 或新 `input_buffer`):`text: String`(含 `\n`)+ `cursor: usize`(字节位,**MUST 落 char 边界**)+ 历史(list / index / draft)。归约 `reduce_*(state, action) -> state`(沿用既有归约风格)。**强制 TDD**。
- 动作:`InsertChar(ch)`、`InsertNewline`、`Backspace`、`Delete`、`MoveLeft`/`Right`、`MoveLineStart`/`End`、`Up`/`Down`(见 D3)、`PushSubmitted`、`SetText(String)`(历史召回/补全写入,cursor 置末尾)。**不含 `InsertStr` 批量粘贴**(→ change B)。
- 行 = 按 `\n` 切;光标行 = cursor 前 `\n` 数;光标列 = 行首到 cursor 的 `display_width`。
- **UTF-8 / 宽字符**:`←→` 按 char 边界步进(不落宽字符中间);`↑↓` 跨**逻辑行**按 **display column** 对齐,目标行取「**≤ 目标视觉列的最大 char 边界**」(落宽字符跨列中间时取左,确定可测);无 goal-column(Non-Goal)。
- **`Up`/`Down` 按逻辑行分派**:reducer 只识 `\n` 切的逻辑行;单条超宽逻辑行虽在渲染时软换行成多显示行,其内部**不支持逐显示行上下移**(在该逻辑行即算首/末行 → 触发 move-其它逻辑行 / 边界翻历史)。v1 限制(见 Non-Goals);reducer 不依赖视口宽,保持纯逻辑。

### D3. `↑↓` 三方优先级:浮层 > 多行内光标 > 边界翻历史(合并动作)
- **接口统一**(finding):app 只投**合并的 `Up`/`Down`** 动作,由 reducer 按 cursor 行位置分派 move vs history(避免 app 层与 reducer 双头判定)。
- 浮层(`models_picker`/`command_completion`)打开时 `↑↓` 归浮层(既有 `arrows_route_to_*` 优先,在投 `Up`/`Down` 之前拦截)。
- reducer `Up`:cursor **不在首行** → 光标上移一行(列对齐);在**首行** → 翻历史上一条(存草稿,cursor 到末尾)。`Down`:不在末行 → 下移;在**末行** → 历史下一条 / 恢复草稿。单行(首行==末行)`Up` 直接翻历史。

### D4. `Home`/`End` 改绑 + 同步 7 条 requirement
- 裸 `Home`/`End`(无 CONTROL)→ 光标到**当前行**首/尾;`Ctrl+Home` → transcript `scroll_to_top`、`Ctrl+End` → `scroll_to_bottom`;`PageUp`/`PageDown` 不变。
- mod.rs `scroll_action_for_key` 改:裸 `Home`/`End` 不返滚动动作(交 on_key 当光标键);`Ctrl+Home`/`Ctrl+End` 返 top/bottom。
- **裸 `Home`/`End` 行内光标移动 MUST NOT 清除 transcript 选区**(它是输入编辑,非滚动);清选区触发里去掉裸 Home/End。
- **spec 同步**:改绑波及 7 条 requirement,全部纳入 delta MODIFIED(`输入历史↑↓`/`transcript 滚动`/`键盘滚动`/`跳到底部`/`鼠标滚轮`/`鼠标拖选`/`文本排版`),保合并主 spec 内部一致;**tasks 点名 3 个断言裸 Home/End→滚动的既有测试**待改。

### D5. 多行渲染 + 动态框高(cap 公式逐项)+ 软换行
- input **内容行数** = `text` 逻辑行经**软换行**(按内框宽)后的显示行数。
- **cap 公式**(按 `layout_rows` 逐项):`cap = clamp(屏高 - (顶栏 3 + status_top_gap(0 或 2) + permission_height(动态) + 活动 1 + 状态 1 + mode 1 + input 边框 2) - transcript_floor, 1, 绝对上限(如 10))`;`transcript_floor = 8`(与 `rows[1] = Min(8)` 口径一致)。**下限 1**:屏高极小时 input 保 **≥1 内容行**(必要时挤占 floor,而非算得 cap=0 让光标/文本无处渲染)。`rows[5]` 高度 = `min(需要显示行, cap) + 2`。
- **内部滚动**:显示行 > cap 时,框内滚动使**光标所在 visual 行**可见。
- **软换行 + 光标定位**:reducer 按逻辑(行,列)算光标;render 对超宽逻辑行软换行,并做 **logical(逻辑行,列) → visual(显示行,列)** 映射,`set_cursor_position` 到 visual 坐标、内部滚动亦按 visual 行。**cursor 落逻辑行软换行满宽边界**(=某显示行末满宽处)时 MUST 归入**下一显示行行首**(唯一确定,供单测)。
- cap / 内部滚动 offset / logical↔visual 映射 MUST 单测(小终端、超宽逻辑行、pending permission 并存)。

### D6. 命令解析仅单行
- `Enter` 提交时:`text` **含 `\n`** → 整段作 prompt 提交(不解析命令);**不含 `\n`** 且以 `/` 起头 → `parse_command`。避免 `/` 开头的多行内容被吞成 Unknown 命令。

### D7. `display_width`/`char_width` 中立化(避免纯逻辑反依赖 render)
- 把二者从 `render.rs` 抽到 `src/tui/width.rs`(`pub(crate)`),render 与文本缓冲 reducer 共用。纯逻辑缓冲不依赖 TUI 外壳。

### D8. 重构爆炸半径:区分读点 / 写点
- **读点**(app.rs `input()` / `command_completion` / `render_input` 读输入串)→ 走新 `text()`。
- **写点**(`complete_selected_command` 直接赋值 `input_line.input`、测试里写 `input`)→ 走 `SetText`(同步 cursor 到末尾),不直接改字段(否则 cursor 不同步)。GREEN 后先 `cargo build` 收敛编译错。

### D9. 模态路由:硬模态吞键 vs 软浮层 command_completion 仍编辑(finding)
- on_key 分流次序保持:`Ctrl+o`/`BackTab` → `pending_permission` → `models_picker` → `command_completion` → 选区 `Ctrl+C`/`Esc` → **新:输入编辑键**。
- **硬模态**(`pending_permission` / `models_picker`)活跃:编辑/换行/光标键 MUST NOT 改文本缓冲(归模态处理)。
- **软浮层 `command_completion`** 活跃(现有 `handle_command_completion_key` 仅吞 `Up/Down/Tab/Enter/Esc`、`Char/Backspace` 落编辑分支重过滤):`Char`/`Backspace` **仍 MUST 改缓冲并触发 `refresh_command_completion`**(否则破坏「继续输入重新过滤」+ 既有绿测 `slash_completion_filters_candidates...`);新光标/换行键(`←→ Home End Delete`/换行)在补全打开时 MUST NOT 破坏过滤(建议忽略或先关浮层)。绝对句「浮层活跃即禁编辑」是错的,只对硬模态成立。
- Tab:v1 不插 `\t`(Tab 仍归命令补全;无补全态 Tab no-op)。

## Risks / Trade-offs

- **[`Ctrl+Enter` 终端依赖]** → 用户 WT 已实测;三键(Ctrl/Shift+Enter、Ctrl+J)冗余,至少一个可用。
- **[宽字符 `↑↓` 列对齐]** → `display_width` + 明确 tie(取 ≤目标列的最大边界);纯逻辑 CJK 单测锁定;无 goal-column 已列 Non-Goal。
- **[动态框高把 transcript 压没 / 越界]** → cap 公式逐项列保 floor;小终端 cap 可能很小(甚至 1 行,光标定位仍 clamp 不 panic);单测覆盖。
- **[input_history 重构冲击]** → 区分读/写点 + `SetText` 同步 cursor;`cargo build` 收敛;迁移既有 input_history 单测。
- **[软换行 logical↔visual 映射错位]** → 集中在 render 的一个映射函数 + 超宽逻辑行快照锁定。

## Open Questions

- 换行键提示:是否在 mode 行提示「Ctrl+Enter 换行」?真机体验后加最小提示。
- `Ctrl+J` 是否稳定 `Char('j')+CONTROL`(不因大小写变 `J`):真机已见 `Char+CONTROL`,实现时匹配 `Char(c) if c.eq_ignore_ascii_case('j') && CONTROL` 更稳。
