# Tasks — fix-shell-clobbers-mouse-capture

## 1. 实现

- [ ] 1.1 `shell.rs`:Windows 下子进程 `creation_flags(CREATE_NO_WINDOW)`(具名常量 `0x0800_0000`,`#[cfg(windows)]`;注释说明防重置 console 输入模式)
- [ ] 1.2 `terminal.rs`:`TerminalGuard::reassert_mouse_capture()`(幂等 `execute!(stdout, EnableMouseCapture)`);`mod.rs` ui_rx 分支:`handle_agent_event` 前以 `matches!` 预判 `ToolCallFinished`,处理后调用重申

## 2. 门禁

- [ ] 2.1 `cargo test --lib` 全绿(既有 run_shell 输出 / exit / 超时测试零回归);`cargo clippy --all-targets -- -D warnings` 零警告
- [ ] 2.2 `openspec validate fix-shell-clobbers-mouse-capture --strict` 通过

## 3. 真机复验(主 agent / 用户)

- [ ] 3.1 采证三步复跑(MYSTERIES_TUI_DEBUG_EVENTS=1):滚轮 → 让 agent 跑 shell 命令 → 滚轮;shell 后滚轮仍产生 `event=mouse Scroll*` 且 transcript 实际滚动、输入框不被触碰
- [ ] 3.2 顺带:粘贴 / 拖选复制 / Esc 中断等既有交互无异常(console 模式相关路径抽查)
