## 1. width 中立化 + 输入文本缓冲 纯逻辑(TDD)

- [x] 1.1 抽 `display_width` / `char_width` 从 `render.rs` 到新 `src/tui/width.rs`(`pub(crate)`),`render.rs` 改用;`cargo build` + 既有 render 快照不变。
- [x] 1.2 **RED**:新建输入缓冲模块(重构 `input_history` 或新 `input_buffer`),只写失败测试:`text: String` + `cursor` 字节位归约——`InsertChar`/`InsertNewline`/`Backspace`/`Delete`、`MoveLeft`/`Right`(char 步进)、`MoveLineStart`/`End`、`Up`/`Down`(reducer 按行位置分派 move vs history)、`SetText`(cursor 置末尾)、`PushSubmitted`;cursor 恒落 char 边界;**宽字符**:`←→` 跨 CJK 整步、`Up`/`Down` 按 `display_width` 列对齐且落宽字符中间取「≤目标列的最大边界」;历史协同:单行 `Up` 直接翻历史、多行内 `Up`/`Down` 移光标、召回后 cursor 到末尾、键入脱离历史、重复提交去重、多行草稿存取。运行确认**失败原因正确**(非编译错),贴测试 + 红输出 → **停,等确认**(新接口首次成型)。
- [x] 1.3 **GREEN**:实现文本缓冲 + `reduce_*(state, action)->state`(复用 `width::display_width`)让 1.2 全绿;`text()` 读访问器、`SetText` 写入口。登记进 `mod.rs`。
- [x] 1.4 迁移:**读点**(app.rs `input()`、`command_completion`、`render_input` 读输入串)改走 `text()`;**写点**(`complete_selected_command` 直接赋值 `input_line.input`、测试里写 `input`)改走 `SetText`(同步 cursor);迁移既有 `input_history` 单测;`cargo build` 收敛编译错。

## 2. 换行 / 提交 / 光标 接线(app.rs on_key)

- [x] 2.1 `on_key` 路由:换行键(`Enter+CONTROL`/`Enter+SHIFT`/`Char('j')+CONTROL`)在**通用 `Char` 分支之前**判定 → `InsertNewline`;通用 `Char` 分支插入前**过滤带 CONTROL 且不带 ALT 的字符**(纯 Ctrl+字符;AltGr=CONTROL+ALT 保留);`Enter`(无 CONTROL/SHIFT)→ 提交,**命令解析仅对不含 `\n` 的 text**(含 `\n` 整段作 prompt);`←→ Home End Backspace Delete` → 光标/编辑动作;`↑↓` → 合并 `Up`/`Down` 动作。补单测(换行不提交、含 `\n` 的 `/` 开头整段作 prompt、Ctrl+J 不插 `j`)。

## 3. 键位改绑(Home/End · ↑↓ 优先级)

- [x] 3.1 `scroll_action_for_key`:**裸** `Home`/`End`(无 CONTROL)不再返滚动动作;`Ctrl+Home` → `scroll_to_top`、`Ctrl+End` → `scroll_to_bottom`;`PageUp`/`PageDown` 不变。**改写 3 个断言裸 Home/End→滚动的既有测试**:`scroll_key_routing_maps_page_and_boundary_keys_only_for_press`、`end_and_ctrl_end_map_to_scroll_to_bottom_and_clear_new_message_count`、`keyboard_boundary_navigation_reaches_top_and_bottom_without_mouse_events`(改为 `Ctrl+Home`/`Ctrl+End`)。
- [x] 3.2 `↑↓` 路由(app.rs `on_key`):**浮层(`arrows_route_to_*`)在投 `Up`/`Down` 之前拦截** > reducer 按 cursor 行分派(多行内移光标 / 边界翻历史);裸 `Home`/`End` 交光标归约。补单测覆盖三方优先级 + 裸 Home/End 不清选区。

## 4. 多行渲染 + 动态框高

- [x] 4.1 纯逻辑:框高 `cap` 公式(逐项:顶栏3 + status_top_gap + permission_height + 活动1 + 状态1 + mode1 + 边框2,保 `transcript_floor=8`)+ `logical(行,列)→visual(行,列)` 软换行映射 + 内部滚动 offset。单测(小终端、超宽逻辑行、pending permission 并存)。
- [x] 4.2 `render_input` 多行渲染 + `set_cursor_position` 到 visual `(行,列)` + 超封顶框内滚动到光标 visual 行;`layout_rows` 的 `rows[5]` 用动态 cap 高度(替 `Length(3)`)。**改写** `input_cursor_position` 及其「光标定位末尾」测试(`input_render_sets_cursor_at_input_end` 或同名)为按 cursor 位置。
- [x] 4.3 `insta` 快照:构造含普通多行 + 一条超宽逻辑行的 `text` + cursor,`TestBackend` 渲染,断言多行/软换行/框高/光标位置/transcript 保底(首个快照人工对 `设计规范/` 审再 approve)。

## 5. 模态路由

- [x] 5.1 编辑/换行/光标键落在既有守卫**之后**,**区分硬模态 vs 软浮层**:`pending_permission` / `models_picker`(硬模态)活跃时编辑/换行/光标键 MUST NOT 进缓冲;`command_completion`(软浮层)活跃时 `Char`/`Backspace` **仍改缓冲并重过滤**(保持既有 `/` 补全,不得回归 `slash_completion_filters_candidates...`),仅 `Up/Down/Tab/Enter/Esc` 归补全。补单测(pending/picker 态换行/字符/光标键不改缓冲;completion 态 `Char` 仍改缓冲重过滤)。

## 6. 校验 + 真机

- [x] 6.1 `cargo test --lib` 全绿 + `cargo clippy --all-targets -- -D warnings` 零警告 + `openspec validate add-multiline-input --strict` 过。
- [x] 6.2 **真机复核**(Windows Terminal):`Ctrl+Enter`/`Shift+Enter`/`Ctrl+J` 换行、`Enter` 提交整段(含 `\n` 的 `/` 开头整段作 prompt);`←→↑↓ Home End` 光标(含 **CJK** 不错位);`Ctrl+Home`/`Ctrl+End` 到顶/底、`PageUp`/`PageDown` 页滚、裸 `Home`/`End` 不滚 transcript;输入框随行长高、超封顶滚动、软换行、**缩窗不崩**、transcript 不被压没;既有选区/滚轮/权限/`/`补全不受影响;退出干净。



