## 1. 鼠标捕获生命周期

- [x] 1.1 `src/tui/terminal.rs`:import `crossterm::event::{EnableMouseCapture, DisableMouseCapture}`;`setup` 改 `execute!(stdout, EnterAlternateScreen, EnableMouseCapture)`;`restore_terminal` 改 `execute!(stdout, DisableMouseCapture, LeaveAlternateScreen)`(`Drop` + panic hook 共用此路径,确保异常退出也解除)。

## 2. 滚轮 → 滚动

- [x] 2.1 `src/tui/mod.rs` 事件循环:`Event::Mouse(me)` 由忽略改为按 `me.kind` —— `ScrollUp` → `scroll_up`、`ScrollDown` → `scroll_down`(每事件 3 行),经既有 `apply_scroll` 取 `total_lines` / `viewport`;其余 kind 忽略。
- [x] 2.2 抽 wheel kind → 滚动原语 / 行数的纯映射函数 + 单测(`ScrollUp`/`ScrollDown` 映射正确、其余 kind 无动作);键盘 `↑↓` 仍走历史不受影响(既有历史测试背书)。

## 3. 文档 + 校验

- [x] 3.1 `设计规范/02-布局与交互.md`:补「鼠标滚轮滚动 transcript」+「框选复制改 Shift+拖选」。
- [x] 3.2 `cargo test --lib` 全绿 + `cargo clippy --all-targets -- -D warnings` 零警告 + `openspec validate enable-mouse-wheel-scroll --strict` 过。

## 4. 真机验证(用户)

- [ ] 4.1 用户在 Windows Terminal 实测:① 滚轮能滚动 transcript;② 键盘 `↑↓` 仍翻输入历史;③ Shift+拖能选择复制。若滚轮仍无效(ConPTY 不转发)→ 反馈,回退或保留仅键盘滚动。
