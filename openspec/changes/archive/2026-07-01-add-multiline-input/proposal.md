## Why

当前输入框只能单行、仅末尾追加/退格,无法手动换行写多行 prompt、无法编辑已输入内容的中间。本 change 给输入框加**多行编辑**:文本缓冲 + 光标导航、`Ctrl+Enter`/`Shift+Enter`/`Ctrl+J` 换行、`Enter` 提交整段、多行渲染 + 输入框动态长高。

**换行键方案已在用户真机(Windows 11 + Windows Terminal)诊断日志实测**:WT 原生就把 `Ctrl+Enter`→`Enter+CONTROL`、`Shift+Enter`→`Enter+SHIFT`、`Ctrl+J`→`Char('j')+CONTROL` 送达,**无需 kitty keyboard protocol**(其 `PushKeyboardEnhancementFlags` 在 Windows 上 `execute!` 返 `Err`,会致 TUI 起不来),故 **`terminal.rs` 不改**,既有选区/滚轮/alt-screen 全不受影响。

**范围说明(本 change 只做「手动多行编辑」)**:用户最初踩的「粘贴多行逐 Enter 狂发」问题**拆到后续 change B** 处理——它需要重构事件循环成「批量 drain」以避免 render 延迟污染 burst 判定,是独立且启发式的一块,单独隔离更稳。本 change 不改事件循环读取结构、不做粘贴突发检测;多行粘贴在本 change 下仍逐 Enter 提交(维持现状,不 regress),由 change B 修复。

## What Changes

- **输入模型重构**:单行 `InputHistoryState.input: String` → **文本缓冲 + 光标**(`text: String` 含 `\n` + `cursor` 字节位,char 边界 + 宽字符感知);历史并入同一模型。归约为纯逻辑、可单测。
- **换行 vs 提交**:`Enter`(无 CONTROL/SHIFT)→ **提交**整段 `text`;`Ctrl+Enter`(`Enter+CONTROL`)/ `Shift+Enter`(`Enter+SHIFT`)/ `Ctrl+J`(`Char('j')+CONTROL`)→ 在光标处插 `\n`。换行键判定 MUST 在通用 `Char` 插入分支**之前**;通用 `Char` 分支 MUST 过滤带 CONTROL 且不带 ALT 的字符(纯 Ctrl+字符,避免 Ctrl+J 插入字面 `j`;AltGr=CONTROL+ALT 保留)。
- **命令解析仅单行**:`Enter` 提交时,仅当 `text` **不含 `\n`** 才走 `parse_command`;含换行的整段一律作为 prompt 提交(避免 `/` 开头的多行粘贴被当命令)。
- **光标导航**:`←→` 按 char 边界步进(跨宽字符整步)、`↑↓` 多行内上下移(视觉列按 `display_width` 对齐)、`Home`/`End`(无 CONTROL)到行首/尾、`Backspace`/`Delete` 编辑。
- **键位改绑**:`↑↓` 优先级 **浮层 > 多行内光标上下移 > 首行↑/末行↓ 翻历史**;裸 `Home`/`End` 从 transcript 滚顶/底**改绑**为行内光标,transcript 顶/底移到 `Ctrl+Home`/`Ctrl+End`(`Ctrl+End` 已是跳到底)。裸 `Home`/`End`(行内光标)**不清除 transcript 选区**。
- **多行渲染 + 动态框高**:`render_input` 多行渲染 + 光标定位到 `(行, display 列)`(替换「恒定位末尾」);输入框高度随内容行数**动态、封顶**(留 transcript 保底),超封顶框内滚动到光标行可见;超宽逻辑行**软换行**,render 做 logical→visual 映射定位光标。
- **共享宽度工具**:把 `render.rs` 私有的 `display_width`/`char_width` 抽到中立模块(`tui/width.rs`),供纯逻辑缓冲 reducer 与 render 共用(避免纯逻辑层反依赖 TUI 层)。
- **模态路由**:编辑/换行/光标键都落在 `pending_permission` / `models_picker` / `command_completion` 既有守卫**之后**。

## Capabilities

### Modified Capabilities

- `tui-shell`:
  - **ADDED**:`多行输入编辑`(文本缓冲+光标纯逻辑归约、换行键、多行渲染+动态框高、光标导航、命令解析仅单行、模态路由)。
  - **MODIFIED**:`输入历史 ↑↓ 召回(本会话内存)` —— `↑↓` 改为「多行内光标上下移,首行↑/末行↓ 才翻历史」。
  - **MODIFIED**:`transcript 滚动` —— 裸 `Home`/`End` 不再滚 transcript,到顶/底改 `Ctrl+Home`/`Ctrl+End`。
  - **MODIFIED**:`键盘滚动全覆盖与鼠标滚轮降级(ConPTY 限制)` —— 键盘到顶/底改 `Ctrl+Home`/`Ctrl+End`。
  - **MODIFIED**:`跳到底部提示与新消息计数` —— 删「`End` 亦可」、滚离底部示例改 `PageUp`/`Ctrl+Home`。
  - **MODIFIED**:`鼠标滚轮滚动(捕获鼠标)` —— 降级备注的键盘滚动键改 `PageUp/PageDown/Ctrl+Home/Ctrl+End`。
  - **MODIFIED**:`鼠标拖选与复制(捕获鼠标下 app 自管选区)` —— 清选区触发的键盘键去掉裸 `Home`/`End`(改 `Ctrl+Home`/`Ctrl+End`),裸 `Home`/`End` 行内光标移动不清选区。
  - **MODIFIED**:`终端文本排版与宽度度量` —— 光标定位从「输入串末尾」改为「cursor 所在 `(逻辑行, display 列)`」。

## Impact

- **依赖**:**无新增**(全用 crossterm/ratatui 既有能力;不引 kitty)。
- **代码**:
  - 输入模型模块(重构 `input_history` → 文本+光标缓冲):纯逻辑核心。
  - `src/tui/width.rs`(新):`display_width`/`char_width` 中立化。
  - `src/tui/app.rs`:`on_key` 路由新键(换行/光标/Ctrl+Home-End)、命令解析仅单行、历史与光标协同;**读点**(`input()` 等)改走 `text()`、**写点**(`complete_selected_command` 等)改走 `set_text` 并同步 cursor。
  - `src/tui/mod.rs`:`Home/End→Ctrl+Home/End` 滚动改绑;动态 `rows[5]` 高度(**不改**事件循环读取结构)。
  - `src/tui/render.rs`:多行渲染 + 光标定位 + 框动态高度(封顶滚动 + 软换行 logical→visual);`layout_rows` input 约束动态。
  - `src/tui/terminal.rs`:**不改**。
- **测试**:文本缓冲/光标/换行/历史协同/宽字符列对齐/框高 cap/命令解析仅单行 纯逻辑 TDD;多行渲染 + 光标 + 动态框高走 `insta` 快照;**改写** `input_cursor_position` 及其「光标定位末尾」测试;**改写** 3 个断言裸 `Home/End`→滚动的既有测试(`scroll_key_routing_maps_page_and_boundary_keys_only_for_press`、`end_and_ctrl_end_map_to_scroll_to_bottom_and_clear_new_message_count`、`keyboard_boundary_navigation_reaches_top_and_bottom_without_mouse_events`)。
- **风险**:① `Ctrl+Enter` 依赖终端转发(用户 WT 已实测),`Shift+Enter`/`Ctrl+J` 兜底;② 宽字符 `↑↓` 列对齐易错 → `display_width` + 明确 tie 规则(取 ≤目标列的最大 char 边界)+ CJK 单测;③ 动态框高 vs `transcript Min(8)` → cap 公式逐项列 + 小终端单测;④ input_history 重构爆炸半径 → 区分读/写点、`cargo build` 收敛。
- **后续**:change B(粘贴防狂发,事件循环批量 drain + burst + pending 态吞突发 Enter)在本 change 之上做。
