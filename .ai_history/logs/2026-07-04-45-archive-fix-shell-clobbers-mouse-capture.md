# 2026-07-04 · 45 · archive-fix-shell-clobbers-mouse-capture

## 决策

- 双层修法:根因 `CREATE_NO_WINDOW`(Windows 子进程不 attach console,连执行中失效窗口都消除)+ 兜底 `ToolCallFinished` 后幂等重发 `EnableMouseCapture` | 弃:仅兜底重申(执行中滚轮仍变方向键误动输入框)、定时重申(无触发依据)、`DETACHED_PROCESS`(语义过重,`CREATE_NO_WINDOW` 已足) | 主导:讨论收敛,用户采证确认根因后放行 | 依据:真机诊断日志两轮(坏:shell 后滚轮 = `key Up/Down` 成对、无 mouse 行;修后:shell 后仍 `event=mouse Scroll*` + `Moved` 完整事件流)
- 根因链:`EnableMouseCapture` 仅启动执行一次(terminal.rs)→ `cmd /C` 子进程 attach console 重置 `ENABLE_MOUSE_INPUT` → Windows Terminal 把滚轮降级为 ↑/↓ 方向键
- **闭案 log 38 悬案**「滚轮偶发操作多行输入框(未复现)」:降级后的方向键落入多行光标 / 输入历史路径——事件在到达前已被平台改性,非 app 错发;log 38 当时诊断到 Mouse 事件正常,是因为那次会话模式尚未被冲掉(触发条件 = 跑过 run_shell)
- 验证策略:平台 IO 胶水不适用红绿(无 Mock 可驱动 console 模式),以真机采证复跑为验收;既有 run_shell 行为测试零回归为门禁

## 变更

- `shell.rs`:`#[cfg(windows)] const CREATE_NO_WINDOW = 0x0800_0000` + cmd 分支 `creation_flags`(sh 分支不受影响)
- `terminal.rs`:`TerminalGuard::reassert_mouse_capture()`(幂等);`mod.rs` ui_rx 分支 `matches!` 预判 `ToolCallFinished` 后调用
- spec:builtin-tools MODIFIED「run_shell 执行」(console 独立性)、tui-shell MODIFIED「键盘滚动全覆盖与鼠标滚轮降级」(捕获重申机制)
- 测试 519 维持(无新增单测,+17 行实现)

## 待决

- 3.2(粘贴 / 拖选 / Esc 抽查)未逐项手测:依据 = Mouse 事件流(Moved/Scroll)完整恢复、本 change 未触碰相关路径、519 零回归;真机日用中若有异常再查
- 未来若新增子进程类工具(如 git 集成),同样需要 `CREATE_NO_WINDOW`;兜底重申已覆盖恢复路径

## 引用

- OpenSpec change:`fix-shell-clobbers-mouse-capture`(propose 33d4ab5 → fix 9575d3d)→ archive/2026-07-04-fix-shell-clobbers-mouse-capture
- 相关 log:[[2026-07-02-38-archive-guard-paste-cross-batch]](悬案出处,本 log 闭案,原文已加批注)、[[2026-07-04-44-archive-add-diff-highlight]](验收过程中复现并采证)
- 跨越 session:本会话(用户真机复现 → 两轮采证 → propose → dispatch 实施 → 主 agent 复审 → 真机复验)
