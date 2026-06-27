## Context

CLI agent 已可跑(`add-cli-assembly` 已 archived,`app::{load_config, select_provider, assemble_agent}` / `cli::run_cli` 就绪)。本 change 是技术方案 §12 step 4 的**第一刀**:引入 §3 双-task + mpsc/oneshot,套 ratatui 最小四区外壳,复用 `app` 装配,跑通一轮。用户已确认 **cut1 不碰 agent-loop**:文本 / 权限走既有 `DeltaSink` / `PermissionDecider` 缝,结构化事件留 cut2。

视觉权威:`theme.rs` / insta(code+tests) > `设计规范/`(text 契约) > 原型 HTML > 推断;`设计规范/02-布局与交互`(四区 + D1 顶栏仅品牌 + D2 Midnight 状态行)、`03-组件清单`(C1/C2/C3/C4/C6/C10/C11)为 cut1 落点。约束:§3 编排离线 Mock 可测;ratatui 走 `TestBackend` + insta 事后快照;不依赖真实终端 / 网络。

## Goals / Non-Goals

**Goals:**

- §3 双-task + channel(cut1 子集协议)+ `ChannelSink` / `ChannelDecider`(复用既有 trait,**不动 agent-loop**)。
- agent-task 一轮编排,Mock 驱动离线可测(含权限 oneshot 往返)。
- ratatui 四区最小外壳(`设计规范/02`),`TestBackend` + insta 验证。
- 终端 RAII 生命周期;main 分流(TUI 默认 / `--headless` cli)。

**Non-Goals(留 cut2 / 后续):**

- 结构化 `AgentEvent`(`ToolCallStarted/Finished` / `StatusChanged`)+ 工具卡 C5 + 全 phase C10 + **MODIFY agent-loop**。
- 全 `01-设计令牌` 主题(`theme.rs` + token 单测)、C6 完整 diff、C7 致命框、滚动、insta 全锁。
- 命令(C8/C9)、markdown、Anthropic、`tool_mode`、step5 收尾。

## Decisions

- **D1 模块布局。** `src/tui/{mod, channel, app, render, terminal}.rs`;`lib.rs += pub mod tui;`。`channel.rs` = 协议 + `ChannelSink` / `ChannelDecider`(走测);`app.rs` = `AppState` + `apply(AgentEvent)` + `on_key`(纯状态,走测);`render.rs` = `render(frame, &AppState)`(走 insta);`terminal.rs` = RAII guard(手动);`mod.rs` = `run_tui`(终端 + spawn agent-task + UI `select!` 循环,手动冒烟)。§4 列 `tui/app.rs render.rs widgets/`,cut1 精简到上述。

- **D2 channel 协议 = §3 子集,不要求 `Clone`。** `enum AgentEvent { TextDelta(String), PermissionRequired(PermissionRequest), TurnComplete, Error(String) }`、`enum UserInput { Prompt(String) }`、`struct PermissionRequest { tool_name, args, responder: oneshot::Sender<PermissionDecision> }`。§3 完整集(`ToolCallStarted/Finished`、`StatusChanged`、`UserEchoed`...)留 cut2 —— 它们需 agent-loop 发事件;cut1 的 UI **本地** echo 用户输入、**本地**推断粗 phase,不依赖这些事件。`AgentEvent` 不 derive `Clone`(被单一 UI task 消费 + 携 oneshot,§3 注)。

- **D3 `ChannelSink` 用 `mpsc::UnboundedSender`。** `DeltaSink::on_text` 是 **sync**;`UnboundedSender::send` 同步非阻塞,契合;`bounded` 的 `send` 是 async、会逼 `on_text` 阻塞或 `try_send` 丢字 —— 故取 unbounded(token 流量小,无背压风险)。备选:bounded + `blocking_send`(弃:agent task 里阻塞运行时)。

- **D4 `ChannelDecider` 跑通 §3 挂起-恢复,零 agent-loop 改动。** `decide`(async)建 `oneshot`、发 `PermissionRequired{responder}`、`rx.await`;UI 断开(`rx` Err)→ `Deny`(fail-safe,呼应既有 gate「通道断=拒绝」)。**关键**:`assemble_agent(provider, &config, Box::new(ChannelDecider))` + `agent.run(.., &ChannelSink)` —— 全部经既有 trait 缝,`agent-loop` 一行不动(验证 D3 划界 + §3 机制)。

- **D5 编排与终端解耦,便于离线测。** 把「持续读 `UserInput`、跑 `Agent.run`、发事件」做成 `run_agent_task(agent, input_rx, ui_tx)`(无终端依赖)→ Mock provider + 程序化投 prompt + 读 `ui_rx` 即可**离线断言一轮**(含权限:对 `PermissionRequired` 回送决策)。`run_tui` 仅在其外包终端 + UI `select!`(crossterm `EventStream` + `ui_rx`),手动冒烟。**理由**:§3 编排是 headless 内核逻辑(走测);终端 IO 不可单测(手动)。

- **D6 终端 RAII + panic hook。** `TerminalGuard`:`new` 启用 raw mode + 进 alternate screen;`Drop` 恢复;另设 panic hook 在 panic 时也恢复(否则 panic 把终端留损坏态)。**理由**:§12 要求「panic 与正常退出都恢复终端」。

- **D7 渲染走 `TestBackend` + insta,首帧人工对眼。** `render(frame, &AppState)` 纯函数式吃状态;测试渲到 `ratatui::backend::TestBackend` 取 buffer → `insta` 快照。**首份快照** `cargo insta review` 人工对 `原型截图/midnight-01-欢迎态.png`(`设计规范/README` 与 config.yaml 强制的唯一人类关卡),approve 后锁定。**非 red-green**(CLAUDE.md:TUI 外壳事后快照)。

- **D8 main 分流,不引 clap。** `main` 查 `env::args` 是否含 `--headless`:是 → 既有 `cli::run_cli(paths, prompt)`;否 → `tui::run_tui(paths)`(prompt 由输入框交互获取)。复用既有 `default_paths()` / `home_dir()`。**理由**:只一个 flag,clap 是未投用表面;`--headless` 留住 headless 路径 + e2e 测,实证 `app` 被两前端复用。(`std::io::IsTerminal` 自动判定为可选增强,cut1 先显式 flag。)

- **D9 cut1 视觉为结构态,不实装全主题。** 港/适配/丢弃见 proposal。`01-设计令牌` 全主题(`theme.rs` + token 单测、Midnight/Daylight)留 cut2;cut1 用最小调色板,首帧快照锁结构(四区 / 品牌 / 欢迎态 / 输入框 / 权限框)。C10 状态行仅 cut1 可观测的粗 phase(就绪 / 忙 / 等待授权);`CallingModel/ExecutingTool` 全 phase 需 cut2 的 `StatusChanged`。**显式标注偏离**(权威次序:surface,不藏)。

- **D10 依赖。** `ratatui` + `crossterm`(`event-stream`,§3 异步键流 → `tokio::select!`)+ `insta`(dev);`tokio` += `sync`(mpsc/oneshot)。`futures-util`(`StreamExt`)已由 transport 引入,复用于 `EventStream.next()`。**不引 clap**。

## Risks / Trade-offs

- **[cut1 工具执行不可见]** 无结构化事件,工具运行仅在 RequiresConfirmation 时经 C6 露面 → 缓解:D9 标注;cut2 加 C5 工具卡 + 事件。骨架 acceptance = 跑通一轮(含权限),非全可视。
- **[终端 / UI `select!` 循环难单测]** → 缓解:D5 把编排逻辑抽离 `run_agent_task`(离线 Mock 测),终端循环仅手动冒烟;D6 RAII 保证恢复。
- **[首帧快照主观]** → 缓解:D7 人工对 `原型截图/`,approve 后由 diff 自动拦漂移;cut1 锁结构、cut2 锁主题。
- **[unbounded channel 无背压]** → 缓解:token / 事件流量小、UI 实时消费;1.0 无背压风险;必要时 cut2 评估 bounded。
- **[跨平台终端]** crossterm 跨平台;raw/alt 在 Windows Terminal / *nix 均支持;CI 无 tty → 默认测不进 `run_tui`(走 `run_agent_task` + `TestBackend`,无需真实 tty)。

## Migration Plan

新增 `tui` 模块 + main 分流;既有 `cli` / `app` / agent-loop **不改**(cli 仅由默认改为 `--headless` 触发)。回滚 = revert 本 change(main 恢复直调 `run_cli`)。无数据迁移。

## Open Questions

- cut2 落地时 `AgentEvent` 如何扩 §3 完整集(agent-loop 发 `ToolCallStarted/Finished` / `StatusChanged`)+ `app.rs` 如何渲工具卡 C5 —— 属 cut2。
- 输入框多行 / 输入历史 / 滚动键位(`02` 标「1.0 实现自定,纳入快照锁定」)—— cut1 先最小(单行提交),细化随 cut2 / 体验 change。
- `ToolContext.max_output_bytes` 与 cwd 在 TUI 下取值(沿用 `cli` 默认)。
