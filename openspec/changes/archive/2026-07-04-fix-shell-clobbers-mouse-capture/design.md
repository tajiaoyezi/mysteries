# Design — fix-shell-clobbers-mouse-capture

## D1 根因链(真机采证 2026-07-04)

`EnableMouseCapture` 在 Windows 上经 `SetConsoleMode` 设置 console 输入模式,仅启动执行一次;`run_shell` 子进程(`cmd /C`)attach 同一 console 后重置该模式;失去 `ENABLE_MOUSE_INPUT` 后 Windows Terminal 把滚轮降级为 ↑/↓ 方向键(alternate-scroll 回退)。诊断日志:shell 前滚轮 = `event=mouse ScrollUp/Down`,shell 后 = `event=key code=Up/Down` 成对出现,无任何 mouse 行。

- 顺带闭案 log 38:「滚轮偶发操作多行输入框」= 降级后的方向键落入多行光标 / 输入历史路径;当时诊断显示 Mouse 事件正常是因为那次会话模式尚未被冲掉(未跑过 shell 或复现条件不同)。

## D2 双层修法

- **根因层**:`shell.rs` 子进程加 `CREATE_NO_WINDOW = 0x0800_0000`(`#[cfg(windows)]`,tokio `Command::creation_flags`;非 Windows 无此问题不加)。子进程不 attach console → 无从重置模式;输出经 pipe 不受影响;交互式命令本就会因 pipe 而不可用,无新增限制。
- **兜底层**:`TerminalGuard::reassert_mouse_capture()`(`execute!(stdout, EnableMouseCapture)`,幂等)+ 事件循环在 ui_rx 处理完 `ToolCallFinished` 后调用(事件在 `handle_agent_event` 前以 `matches!` 预判,避免所有权问题)。
- **为何两层**:根因层消除执行中窗口期与误操作输入框;但其有效性依赖平台行为推断,兜底层用观测到的失效模式(模式丢失)做恢复,并覆盖未来其他子进程类工具。两层各 ~3 行,均幂等。
- **被否**:仅兜底重申——命令执行期间滚轮仍会变方向键误动输入框;定时重申——无触发依据,浪费写放大;换用 `DETACHED_PROCESS`——语义更重(子进程完全无 console 概念),`CREATE_NO_WINDOW` 已足够且更常规。

## D3 验证策略

- 平台 IO 胶水:不适用红绿(无 Mock 可驱动 console 模式);既有 `run_shell` 行为测试(输出 / exit / 超时)与全量测试零回归为门禁。
- 真机复验 = 采证三步复跑:滚轮 → 跑 shell 命令 → 滚轮;两段滚轮都应产生 `event=mouse Scroll*` 日志且 transcript 实际滚动;输入框不被滚轮触碰。
- 重申调用点不做单测(需终端副作用 seam,收益低);以真机与代码 review 把关。
