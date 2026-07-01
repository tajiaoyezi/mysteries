## 1. 依赖 + theme token

- [x] 1.1 `Cargo.toml` 新增 `arboard`(理由:本机直写系统剪贴板、绕开 OSC52 在 WT 不稳;**用户已批准**);`cargo build` 过;`cargo tree -p arboard` 复核传递依赖足迹(Windows 下预期 `clipboard-win`/`windows-sys` 为主),异常膨胀则回报。
- [x] 1.2 `src/tui/theme.rs`:`Theme` 加 `selection_bg` 字段 + `midnight()` / `daylight()` 各给值 + `tokens()` 数组 `19→20`;更新两个 token 单测(test-after,`设计规范/01` 对照)锁双调色板值。

## 2. selection.rs 选区纯逻辑(TDD 红→绿)

- [x] 2.1 **RED**:新建 `src/tui/selection.rs`,只写失败测试:`reduce_selection` 起选 / 扩选 / **单击清除(`Press`→`Release` 无位移→`None`)/ 有位移 `Release` 保留选区** / `Press` 覆盖旧选区、`Selection::normalized()` 乱序→start≤end、`col_range_for_row` 单行 / 跨行首中尾。运行确认**失败原因正确**(非编译错),贴测试 + 红输出 → **停,等确认**(新接口首次成型)。
- [x] 2.2 **GREEN**:`Point` / `Selection` / `SelectionState` / `SelectionAction` + `reduce_selection` / `normalized` / `col_range_for_row` 最小实现让 2.1 全绿;`pub mod selection;` 登记进 `mod.rs`。
- [x] 2.3 `src/tui/app.rs`:`AppState` 加 `pub selection: SelectionState` 字段(`with_session_and_history` / `Default` 初始化)+ 薄方法 `apply_selection_action` / `clear_selection` / `has_selection`;提交(`on_key` Enter)与 `/clear` 执行处调 `clear_selection`,补单测。

## 3. 选区取文纯函数(TDD 红→绿,按显示宽度跳延续格)

- [x] 3.1 **RED**:`src/tui/render.rs` 写 `selection_text(buffer, sel)` 的失败测试——**用 `Buffer::set_string` 真实写入含 CJK 宽字符 + 尾随补白的行**(延续格 symbol 实为 `" "`),断言取出「你好」**两 CJK 间无多余空格**、逐行 `trim_end`、跨行 `\n` join、单行取 `start.col..=end.col`;另测选区越界坐标不 panic。确认失败原因正确。
- [x] 3.2 **GREEN**:实现 `selection_text`(复用 `selection::col_range_for_row`;**按 cell symbol 显示宽度推进、跳 `width-1` 个延续格,不用 `is_empty()`**;cell 访问走 `Buffer::cell` 的 `Option` 短路 / clamp)让 3.1 全绿。

## 4. Clipboard trait + 复制副作用(TDD 红点)

- [x] 4.1 **RED**:定义 `trait Clipboard { fn set_text(&mut self, text: String) -> Result<(), String>; }`;写 `copy_selection` 接线的失败测试(注入 mock):成功路径断言 `set_text` 收到 `selection_text` 结果且**复制后选区保留**(不清除);失败路径断言发一条复制失败 `Notice`、不 panic、**选区保留**;**空 / 纯空白选区或 `last_frame` 为 `None` 时不调用 `set_text`**。贴测试 + 红输出 → **停,等确认**(新 trait 首次成型)。
- [x] 4.2 **GREEN**:实现 `ArboardClipboard`(包 `arboard::Clipboard`,`new` 失败→降级态)+ `copy_selection`(读 `last_frame` buffer → `selection_text`;`trim` 后为空 / `last_frame` 为 `None` → 跳过不触剪贴板;否则 `Clipboard::set_text`;`Err`/无 clipboard → `AgentEvent::Notice`;**成功与失败均不清选区**)让 4.1 全绿。

## 5. 事件循环接线(mod.rs)

- [x] 5.1 `Event::Mouse(me)` 分流:`ScrollUp/Down` 走既有滚轮**并清选区**;`Down/Drag/Up(Left)` 透传 `me.column/me.row` 映射为 `Press/Drag/Release` 调 `state.apply_selection_action(...)`;`Up(Left)` 归约后仍有选区 → `copy_selection`(不清选区);其余 kind 忽略。改掉现只传 `me.kind` 的写法。
- [x] 5.2 `Ctrl+C` / `Esc` 分流:**优先级 `pending_permission`(模态)> 选区 > 中断/退出**——pending 时 `Esc`→拒授权、`Ctrl+C`→维持原(no-op),不因选区改变;无 pending 有选区时 `Ctrl+C`→`copy_selection`(不退出不清)、`Esc`→`clear_selection`(不退出不中断);无 pending 无选区维持原 `should_exit`。选区拦截落在既有 pending/浮层守卫**之后**。补 `should_exit`-风格纯逻辑测覆盖优先级(pending>选区>运行中>就绪)。
- [x] 5.3 **键盘滚动 + resize 清选区**:`handle_scroll_key`(`PageUp/PageDown/Home/End/Ctrl+End`)路径调 `clear_selection`;`Event::Resize` 从 no-op 改为 `clear_selection`。补测清选区触发覆盖(键盘滚动 / resize)。
- [x] 5.4 事件循环持 `last_frame: Option<Buffer>`:每次 `terminal.draw(...)` 拿 `CompletedFrame`,选区活跃时 `.buffer.clone()` 存入(**不**用 `current_buffer_mut()`);`copy_selection` 读之。
- [x] 5.5 复核 `terminal.rs` 保持 alt-screen + `EnableMouseCapture`(无改动,确认);`arboard` 写入不影响 `restore_terminal` 单一恢复路径。

## 6. 选区高亮渲染(render.rs)+ 快照

- [x] 6.1 `render` 末尾追加 `highlight_selection(frame, state, theme)` overlay pass:遍历选区覆盖 cell,置对应 cell `bg = theme.selection_bg`(仅背景,留前景)。**cell 写入用 `Buffer::cell_mut`(`Option`)/ clamp 到 `buffer.area`,禁止裸 `Index`**;限当前可见视口。
- [x] 6.2 `insta` 快照:构造带**定稿(已 Release)**选区的 `AppState`,`TestBackend` 渲染,断言高亮 cell 背景为 `selection.bg`、前景不变、松开后高亮仍在(首个快照人工对 `设计规范/` 审一次再 approve)。

## 7. 校验 + 真机

- [x] 7.1 `cargo test --lib` 全绿 + `cargo clippy --all-targets -- -D warnings` 零警告 + `openspec validate fullscreen-mouse-select-copy --strict` 过。
- [x] 7.2 **真机复核**:全屏布局不变;滚轮滚 transcript(有选区则清);**不按 Shift** 拖选 → 松开即复制、高亮保留、粘贴校验(含 **CJK 无多余空格**);有选区 `Ctrl+C` 复制不退出、无选区 `Ctrl+C` 退出;`Esc` 有选区先清选区、pending 授权时 `Esc` 仍拒授权(模态优先);键盘 `PageUp/Home/End` 滚动后选区清、无错位高亮;**选中文本后缩小终端窗口不崩**;拖到空白区松开不覆盖剪贴板;`↑↓` 翻输入历史不受影响;退出 / panic 终端恢复干净;人为制造复制失败降级为 Notice 不崩。
