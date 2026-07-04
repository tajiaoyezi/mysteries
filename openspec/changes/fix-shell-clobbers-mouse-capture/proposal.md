# fix-shell-clobbers-mouse-capture

## Why

真机采证(MYSTERIES_TUI_DEBUG_EVENTS)坐实:`run_shell` 执行**之前**滚轮正常(Mouse ScrollUp/Down 到达);执行**之后**滚轮到达应用的是 `Key Up/Down` 的 Press/Release 对——鼠标事件不再上报。根因链:`EnableMouseCapture` 仅在启动时执行一次(terminal.rs:25);`run_shell` 的 `cmd /C` 子进程 attach 到同一 console,重置输入模式(`ENABLE_MOUSE_INPUT` 丢失);Windows Terminal 随即把滚轮**降级翻译为 ↑/↓ 方向键**。

这同时解开 log 38 的悬案「滚轮偶发操作多行输入框(未复现)」:滚轮变方向键后,落进多行光标 / 输入历史路径——不是 app 逻辑错发,是事件在到达前已被平台改性。观察池该项由本 change 关闭。

## What Changes

1. **根因**:Windows 下 `run_shell` 子进程以 `CREATE_NO_WINDOW`(0x08000000)创建——不 attach 调用方 console,从源头杜绝输入模式被重置(连命令执行中的失效窗口期都消除);stdout/stderr 本就经 pipe 捕获,不受影响。
2. **兜底**:TUI 事件循环在收到 `ToolCallFinished` 后幂等重发 `EnableMouseCapture`(`TerminalGuard::reassert_mouse_capture`)——防未来其他子进程类路径重现同类冲击;重复启用无害。

## Impact

- Affected specs: `builtin-tools`(MODIFIED run_shell:子进程 console 独立性)、`tui-shell`(MODIFIED 键盘滚动全覆盖与鼠标滚轮降级:捕获重申机制)
- Affected code: `src/tool/shell.rs`(creation_flags,`#[cfg(windows)]`)、`src/tui/terminal.rs`(重申方法)、`src/tui/mod.rs`(ToolCallFinished 后调用)
- 平台 IO 胶水,不适用红绿 TDD;验收以真机复跑采证三步为准;既有测试零回归
- headless 不受影响(无终端可冲;CREATE_NO_WINDOW 对 pipe 捕获无影响)
