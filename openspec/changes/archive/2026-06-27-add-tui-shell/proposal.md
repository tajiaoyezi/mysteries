## Why

CLI agent 已可跑(`add-cli-assembly`),但交互仍是单次 prompt → stdout 流。技术方案 §12 step 4 = 套 ratatui、引入 §3 双-task + channel 架构,把 stdin/stdout 换成 TUI。这是**第一刀(骨架)**:立起 §3 的 agent-task ↔ UI-task + mpsc/oneshot 缝,搭最小四区外壳(`设计规范/02-布局与交互`),**全程复用 `app::{load_config, select_provider, assemble_agent}`**,跑通一轮 输入→响应。完整主题 / 结构化事件 / 工具卡留第二刀(`add-tui-rich`),详见拆分。

## What Changes

- **新依赖与 `tui` 模块**:`ratatui` + `crossterm`(`event-stream`)+ `insta`(dev);`tokio` += `sync` feature(mpsc/oneshot)。新建 `src/tui/{mod, channel, app, render, terminal}.rs`。
- **§3 双-task + channel(cut1 子集协议)**:
  - UI→Agent:`UserInput::Prompt(String)`(cut1 只 Prompt;`Command` 留命令 change)。
  - Agent→UI:`AgentEvent`(cut1 子集:`TextDelta` / `PermissionRequired`(携 `oneshot`)/ `TurnComplete` / `Error`)。结构化 `ToolCallStarted/Finished` / `StatusChanged` 留 cut2(需 agent-loop 发事件)。
- **`ChannelSink`**(impl `DeltaSink`):`on_text` → `mpsc::UnboundedSender<AgentEvent>::send(TextDelta)`(unbounded send 同步,契合 sync `on_text`)。
- **`ChannelDecider`**(impl 既有 async `PermissionDecider`):`decide` → 建 `oneshot`、发 `PermissionRequired{tool, args, responder}` 给 UI、`rx.await` 取决策;UI 端断开 → `Deny`(fail-safe,呼应既有 gate)。**这把 §3 的「挂起-恢复」机制跑通,且不动 agent-loop**。
- **agent-task 编排**:`assemble_agent(provider, &config, Box::new(ChannelDecider))` + `tokio::spawn` 跑 `agent.run(&mut history, &ctx, &ChannelSink)`;事件经 channel 流回 UI。
- **ratatui 四区外壳**(`设计规范/02`):顶栏(C1)/ transcript(C2 欢迎态 + C3 user echo + C4 文本流)/ 权限框内联(C6 机制)/ 状态行(C10,**粗** Idle/Busy/WaitingForPermission)/ 输入框(C11)。crossterm `EventStream` + `tokio::select!`(§3 UI 事件循环)。
- **终端生命周期**:RAII guard 进 raw mode / alternate screen,**panic 与正常退出都恢复**(panic hook + Drop)。
- **main 分流**:默认进 TUI;`--headless` → 既有 `cli::run_cli`(**不删 cli**,留 headless 路径 + e2e 测,实证 `app` 被两前端复用)。复用 main 既有 `default_paths()` / `home_dir()`。

### 4 点定夺(已与你确认 cut1 不碰 agent-loop)

1. **拆分**:cut1 = §3 骨架 + 最小外壳(本 change);cut2 `add-tui-rich` = 结构化 `AgentEvent`(**MODIFY agent-loop**)+ 工具卡(C5)+ 全 phase(C10)+ 全 `01-设计令牌` 主题(`theme.rs` + token 单测)+ C6 完整 diff + C7 致命框 + 滚动 + insta 全锁。命令(C8/C9)、markdown(1.4)、Anthropic、step5 收尾更后。
2. **事件观测面**:cut1 **不扩 Agent 事件接口、不动 agent-loop** —— 文本走 `DeltaSink→ChannelSink`、权限走 `PermissionDecider→ChannelDecider`(oneshot)。**capability 影响:NEW `tui-shell`,零 modified**。代价:cut1 工具执行「不可见」(仅 RequiresConfirmation 时弹 C6),工具卡 C5 留 cut2。
3. **测试分界**:见 design / tasks —— §3 编排(channel/`ChannelSink`/`ChannelDecider`/agent-task 一轮)= Mock 驱动**离线 red-green 走测**;ratatui 渲染(四区/C1/C2/C6/C10/C11)= `TestBackend` + `insta` **事后快照**,首帧人工对眼 `原型截图/midnight-01-欢迎态.png`;终端生命周期 + main 分流 = 手动 / RAII(非测)。
4. **main 分流 + deps**:默认 TUI、`--headless`→cli(不引 clap,`env::args` 查 flag);deps 如上,justify 见 Impact。

**港/适配/丢弃(port/adapt/drop · cut1,`设计规范/README` 强制)**:
- **port ✅**:`02` 四区布局分区、C1 顶栏品牌文本、C3 `>` user marker、C4 文本流、C6 权限框位置 / 结构、信息层级。
- **adapt ⚠️**:圆角 → box-drawing 边框;按钮 → `[y·允许][n·拒绝]` 文本;C11 placeholder 文本;truecolor → **cut1 用最小调色板**。
- **drop ❌**:阴影 / 渐变 / 鼠标 hover / 补间动画;spinner(留 cut2,cut1 状态行用静态 label)。
- **显式偏离**:cut1 **不**实装全 `01-设计令牌` 主题(`theme.rs` + token 单测留 cut2),首帧快照为**结构态**(最小色);C10 状态行仅 cut1 可观测的粗 phase(全 `CallingModel/ExecutingTool` 需 cut2 事件)。

**明确不含**(留后续):cut2 的全部(结构化事件 / 工具卡 / 全主题 / 滚动 / C6 diff / C7 框)、内置命令(C8/C9,§8)、Anthropic、`tool_mode` 降级、step5 收尾。

## Capabilities

### New Capabilities

- `tui-shell`: ratatui TUI 骨架 —— §3 双-task + mpsc/oneshot channel、`ChannelSink` / `ChannelDecider`(复用既有 `DeltaSink` / `PermissionDecider`)、agent-task 编排、四区最小外壳渲染、终端生命周期、main 分流(TUI 默认 / `--headless` cli)。

### Modified Capabilities

<!-- 无。cut1 不碰 agent-loop（结构化事件留 cut2）、不改 cli-runtime 的 requirement（run_cli 不变,仅经 --headless 条件调用）。零 modified capability。 -->

## Impact

- **新增代码**:`src/tui/{mod, channel, app, render, terminal}.rs`;`src/lib.rs` += `pub mod tui;`;`src/main.rs` 加 `--headless` 分流。
- **新增依赖**(§11):`ratatui`、`crossterm`(`event-stream`,§3 异步键流)、`insta`(dev,§10 快照);`tokio` += `sync`(mpsc/oneshot)。`futures-util`(`StreamExt` for `EventStream`)**已由 transport 引入**,复用。**不引 clap**。justify:TUI 库 CLAUDE.md 明许三方;channels 需 tokio `sync`;快照需 insta。
- **构建 / 测试**:`cargo build` 通过;§3 编排 / `ChannelSink` / `ChannelDecider` / 一轮往返走**离线 Mock TDD**;ratatui 渲染走 `insta`(首帧人工对眼)。`cargo test` 默认不需终端、不触网。
- **里程碑**:本 change 后 `cargo run` 进 TUI、跑通一轮 输入→响应(含 RequiresConfirmation 的 oneshot 权限框);`cargo run -- --headless "<prompt>"` 仍走原 CLI。
- **下游契约**:`tui` 模块 + channel 协议供 cut2 扩展(加结构化事件 / 工具卡 / 主题);验证了 `app` 装配被 CLI / TUI 两前端复用(D3 划界成立)。
