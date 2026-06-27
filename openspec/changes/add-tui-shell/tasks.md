## 1. 依赖 + tui 模块骨架

- [x] 1.1 `Cargo.toml` 加 `ratatui`、`crossterm`(`features = ["event-stream"]`)、`insta`(dev);`tokio` features += `sync`;选版本记由(§11)。`cargo build` 通过
- [x] 1.2 建 `src/tui/{mod, channel, app, render, terminal}.rs`;`src/lib.rs` += `pub mod tui;`;`cargo build`(空骨架)

## 2. channel 协议类型(§3 cut1 子集)

- [x] 2.1 在 `tui/channel.rs` 定义 `AgentEvent { TextDelta(String), PermissionRequired(PermissionRequest), TurnComplete, Error(String) }`(**不** derive `Clone`)、`UserInput { Prompt(String) }`、`PermissionRequest { tool_name: String, args: Value, responder: oneshot::Sender<PermissionDecision> }`(见 design D2)
- 注:纯类型,不走 red-green;由 §3–§5 测试钉死

## 3. ChannelSink(走测 · Mock 驱动)

- [x] 3.1 【红】写测试:持 `mpsc::UnboundedSender` 的 `ChannelSink::on_text("hello")` → 接收端收 `AgentEvent::TextDelta("hello")`;确认失败
- [x] 3.2 【绿】实现 `ChannelSink`(impl `DeltaSink`,`UnboundedSender::send(TextDelta)`,见 design D3)
- [x] 3.3 【重构】清理

## 4. ChannelDecider(走测 · oneshot 往返 · 停点[新权限流])

- [x] 4.1 【红 · 停点】写测试:`decide` 发 `PermissionRequired`、UI 经 `responder` 回 `Allow` → `decide` 返回 `Allow`;responder 丢弃(`rx` Err)→ 返回 `Deny`(fail-safe);确认失败。**贴 `ChannelDecider`/`PermissionRequest` 草案 + 失败输出,停下等确认**(新权限流首次成型)
- [x] 4.2 【绿】实现 `ChannelDecider`(impl 既有 async `PermissionDecider`,`oneshot` + `rx.await`,断开→`Deny`,见 design D4)
- [x] 4.3 【重构】清理

## 5. run_agent_task 编排(走测 · Mock 一轮 · 无终端)

- [x] 5.1 【红】写测试(Mock provider 脚本「轮1 RequiresConfirmation 工具 tool_call、轮2 终复」+ tempdir cwd):程序化投 `UserInput::Prompt`、对收到的 `PermissionRequired` 回送 `Allow` → 断言 channel 见文本/权限事件、工具执行、`TurnComplete`;确认失败
- [x] 5.2 【绿】实现 `run_agent_task(agent, input_rx, ui_tx)`(读 `UserInput`、`assemble_agent` 出的 agent 经 `ChannelSink`/`ChannelDecider` 跑 `Agent.run`、发事件,见 design D5)
- [x] 5.3 【重构】清理

## 6. AppState(走测 · 纯状态)

- [x] 6.1 【红】写测试:`apply(TextDelta)` 累积到当前 assistant 块;`apply(PermissionRequired)` 置 pending + 粗 phase=等待授权;`on_key` 文本编辑 / 回车提交产 `UserInput::Prompt` / `y`·`n` 答 pending 权限;确认失败
- [x] 6.2 【绿】实现 `AppState`(transcript 块 + 输入缓冲 + 粗 phase + pending 权限)、`apply(&AgentEvent)`、`on_key`(见 design D2/D9)
- [x] 6.3 【重构】清理

## 7. 终端生命周期 + ratatui 渲染(终端=手动 / 渲染=走 insta · 首帧人工对眼停点)

- [x] 7.1 实现 `TerminalGuard`(`new` 启 raw + alt screen,`Drop` 恢复)+ panic hook 恢复(手动验证 panic 与正常退出都还原终端,见 design D6)
- [x] 7.2 实现 `render(frame, &AppState)`:`设计规范/02` 四区 —— 顶栏 C1(品牌)/ transcript(C2 欢迎态 + C3/C4 块)/ 状态行 C10(粗 phase)/ 输入框 C11;pending 时 C6 权限框内联(见 design D7/D9 港·适配·丢弃)
- [x] 7.3 【insta · 停点】写 `TestBackend` 快照测:欢迎态(空会话)+ 权限态(pending);`cargo insta review` **首帧人工对 `原型截图/midnight-01-欢迎态.png` 对眼**,approve 后锁定。**贴首帧渲染给用户审**(config.yaml + `设计规范/README` 强制的人类关卡)

## 8. main 分流 + run_tui + 收尾

- [x] 8.1 `tui::run_tui(paths)`:`TerminalGuard` + spawn `run_agent_task` + UI `select!`(crossterm `EventStream.next()` ↔ `ui_rx.recv()`)→ `app.apply` / `app.on_key` → `terminal.draw(render)`(手动冒烟,见 design D5/D8)
- [x] 8.2 `src/main.rs` 分流:`env::args` 含 `--headless` → 既有 `cli::run_cli`;否则 `tui::run_tui`(复用 `default_paths()`/`home_dir()`,不引 clap,见 design D8);`cargo run` 进 TUI、`cargo run -- --headless "<p>"` 走 CLI —— 冒烟
- [x] 8.3 收尾:`cargo build`、`cargo test` 默认全绿(§3 编排走测 + insta 快照,无终端/不触网)、`cargo fmt`;自检:`tui-shell` spec 7 条 requirements 有落点(走测 / insta / 手动 已分类);偏离已标注(D9 结构态非全主题 + 粗 phase、cut1 不碰 agent-loop);港/适配/丢弃已在 proposal 记录
