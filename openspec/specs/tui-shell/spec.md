# tui-shell Specification

## Purpose
TBD - created by archiving change add-tui-shell. Update Purpose after archive.
## Requirements
### Requirement: §3 双-task + channel 协议

系统 SHALL 以技术方案 §3 的双-task 架构运行 TUI:一个 agent task(跑 `Agent.run`)与一个 UI task(渲染 + 事件)经 channel 通信。UI→Agent 用 `UserInput`(cut1 至少 `Prompt(String)`);Agent→UI 用 `AgentEvent`(cut1 子集:`TextDelta` / `PermissionRequired`(携 `oneshot::Sender<PermissionDecision>`)/ `TurnComplete` / `Error`)。`AgentEvent` MUST NOT 要求 `Clone`(被单一 UI task 独占消费,且 `PermissionRequired` 携不可 `Clone` 的 oneshot)。

#### Scenario: 一轮 prompt 经 channel 往返

- **WHEN** 以 Mock provider(脚本:一段文本回复)装配 agent task,向其投入 `UserInput::Prompt`
- **THEN** UI 端从 channel 依次收到 `TextDelta`(一或多段)与 `TurnComplete`,全程无需终端

### Requirement: ChannelSink 文本增量转发

`ChannelSink` SHALL impl 既有 `DeltaSink`,其 `on_text` MUST 把文本增量经 `mpsc::UnboundedSender<AgentEvent>` 以 `TextDelta` 推出(unbounded 同步 send,契合 sync `on_text`,不阻塞 agent task)。

#### Scenario: on_text 推出 TextDelta

- **WHEN** 对一个持 channel sender 的 `ChannelSink` 调用 `on_text("hello")`
- **THEN** channel 接收端收到 `AgentEvent::TextDelta("hello")`

### Requirement: ChannelDecider 权限 oneshot 往返

`ChannelDecider` SHALL impl 既有 async `PermissionDecider`:`decide` MUST 创建 `oneshot`、向 UI 发 `AgentEvent::PermissionRequired{tool_name, args, responder}`、在 `responder` 的 `rx.await` 处挂起,收到决策后返回之;若 UI 端 sender / responder 断开,MUST 返回 `PermissionDecision::Deny`(fail-safe)。本机制 MUST 不改动 `agent-loop`(经既有 `PermissionDecider` 缝接入)。

#### Scenario: 权限请求挂起-恢复

- **WHEN** `ChannelDecider::decide` 被调用,UI 收到 `PermissionRequired` 后经 `responder` 回送 `Allow`
- **THEN** `decide` 返回 `Allow`(挂起在 `rx.await`、收到后恢复)

#### Scenario: UI 断开 fail-safe 拒绝

- **WHEN** `decide` 发出请求后 UI 端 responder 被丢弃(`rx` 出错)
- **THEN** `decide` 返回 `Deny`,不 panic

### Requirement: agent-task 一轮编排(Mock 驱动 · 无终端)

系统 SHALL 提供可在**无终端**下以 Mock provider 驱动的 agent-task 编排:投入一个 prompt,经 `ChannelSink`(文本)与 `ChannelDecider`(权限)跑完一轮 `Agent.run`,把事件流回 channel。含 `RequiresConfirmation` 工具的脚本 MUST 能走通「`PermissionRequired` → 回送决策 → 继续 / 拒绝入 history」。

#### Scenario: 含权限的一轮编排

- **WHEN** Mock 脚本为「轮1 → 一个 RequiresConfirmation 工具的 tool_call、轮2 → 终复文本」,投入 prompt 并对 `PermissionRequired` 回送 `Allow`
- **THEN** channel 依次见到权限请求与文本事件,工具被执行,最终 `TurnComplete`;全程无终端、不触网

### Requirement: ratatui 四区最小外壳渲染

系统 SHALL 用 ratatui 渲染 `设计规范/02-布局与交互` 的四区布局:顶栏(C1,仅品牌 `✦ mysteries  agent · v1.0`)/ transcript(空会话 → C2 欢迎态;有会话 → user/assistant 文本块)/ 状态行(C10,cut1 粗 phase:就绪 / 忙 / 等待授权)/ 输入框(C11,`mysteries ▸ ` + 占位)。`PermissionRequired` pending 时,C6 权限框 MUST 内联钉在状态行上方。渲染 MUST 可经 `ratatui::backend::TestBackend` 快照验证(`insta`),首帧人工对 `原型截图/midnight-01-欢迎态.png` 审核。

#### Scenario: 欢迎态结构快照

- **WHEN** 空会话状态渲染到 `TestBackend`
- **THEN** 快照含 顶栏品牌行、C2 欢迎态(wordmark + 标语 + 建议行)、状态行、输入框占位四区结构,且与锁定的 `insta` 快照一致

#### Scenario: 权限态内联框

- **WHEN** 存在一个 pending 的 `PermissionRequired`(工具名 + args)时渲染
- **THEN** 快照在状态行上方含 C6 权限框(`▲ 需要授权` + 工具名/args + `[y·允许][n·拒绝]`),与锁定快照一致

### Requirement: 终端生命周期恢复

系统 SHALL 以 RAII guard 管理终端:进入时启用 raw mode + alternate screen,**正常退出与 panic 都 MUST 恢复**(离开 alternate screen、关 raw mode),避免把用户终端留在损坏态。

#### Scenario: 退出恢复终端

- **WHEN** TUI 正常退出或 agent / UI task panic
- **THEN** 终端被恢复(raw mode 关闭、回到主屏),不残留损坏态

### Requirement: main 分流(TUI 默认 / headless 回退)

`main` SHALL 默认进入 TUI;当传入 `--headless` 时 MUST 改走既有 `cli::run_cli`(headless 路径与其 e2e 测保留)。两路 MUST 复用 `app::{load_config, select_provider, assemble_agent}`(同一装配,不同前端)。

#### Scenario: headless 回退到 CLI

- **WHEN** 以 `--headless` 启动并给定 prompt
- **THEN** 走 `cli::run_cli`(stdout 流),不进入 ratatui

#### Scenario: 默认进入 TUI

- **WHEN** 不带 `--headless` 启动
- **THEN** 进入 ratatui TUI(四区外壳),prompt 由输入框交互获取

### Requirement: 结构化事件经 ChannelObserver 上送

`tui` 的 `AgentEvent` SHALL 扩展 `ToolCallStarted{id, name, args, readonly}` / `ToolCallFinished{id, outcome}` / `StatusChanged(AgentStatus)`。系统 SHALL 提供 `ChannelObserver`(impl `AgentObserver`),把观测回调 forward 成对应 `AgentEvent` 经 `mpsc::UnboundedSender` 上送(mirror 既有 `ChannelSink` / `ChannelDecider`);`run_agent_task` MUST 改调 `Agent::run_observed(.., &ChannelSink, &ChannelObserver)`,使文本与结构化事件经同一 channel 流回 UI。

#### Scenario: 工具轮的结构化事件流(Mock · 无终端)

- **WHEN** 以 Mock 脚本(含一个工具的 tool_call)驱动 `run_agent_task`,对权限请求回送 `Allow`
- **THEN** channel 依次收到 `StatusChanged(CallingModel)` → `ToolCallStarted` → `ToolCallFinished` → 后续文本 → `TurnComplete`,全程无终端

### Requirement: 工具卡 C5 渲染

`AppState` SHALL 据 `ToolCallStarted` / `ToolCallFinished` 维护工具卡块;`render` SHALL 按 `设计规范/03` C5 渲染:头(状态 glyph `running`→占位 / `done`→`✓` / `error`→`✗` + 工具名 + args;只读工具带 `只读 · 自动运行` 徽章)、体(`output` 行;截断时 `⋯ +N 行已截断`)、脚(`exit {code}`)。本 change 为**结构态**(最小色,主题留 cut2b;`running` 用静态字符,spinner 留 cut2b)。

#### Scenario: 工具卡三态结构快照

- **WHEN** 分别以 running / done / error 态的工具卡渲染到 `TestBackend`
- **THEN** `insta` 快照含 C5 结构(glyph + 名 + args + 只读徽章 + output + exit + 截断标记),且与锁定快照一致

### Requirement: 全 phase 状态行 C10

状态行 SHALL 据 `StatusChanged` 显示完整 phase(`设计规范/02` 状态机):`Idle`→`◇ 就绪`、`CallingModel`→`调用模型…`、`ExecutingTool(name)`→`执行 {name}…`、`WaitingForPermission`→`▲ 等待授权…`(替换 cut1 的粗 phase)。`AppState` 的 phase 状态 MUST 可单测,渲染 MUST 可 `insta` 快照。

#### Scenario: phase 随事件更新(状态可测)

- **WHEN** `AppState.apply(StatusChanged(ExecutingTool("write_file")))`
- **THEN** 其 phase 为 `ExecutingTool("write_file")`,后续渲染状态行左侧显示 `执行 write_file…`

#### Scenario: 各 phase 状态行快照

- **WHEN** 分别以 `Idle` / `CallingModel` / `ExecutingTool(x)` / `WaitingForPermission` 渲染状态行
- **THEN** 各自 `insta` 快照与锁定一致(glyph + label 正确)

### Requirement: 主题令牌 theme.rs(双调色板)

系统 SHALL 提供 `theme.rs`:`Theme` 结构持 `设计规范/01-设计令牌` 的全部语义 token(背景 / 描边 / 文字 / `accent.primary` / `success.fg` / `warning.fg` / `warning.bg` / `error.fg` / `error.bg` / `error.border` / `info.fg`),并提供 `Theme::midnight()` 与 `Theme::daylight()` 两套调色板,值为 `设计规范/01` 表的 `Color::Rgb`。token 值 MUST 由单测锁定(配色漂移 = 测试红)。

#### Scenario: token 值单测锁定

- **WHEN** 取 `Theme::midnight()` 与 `Theme::daylight()`
- **THEN** 各语义 token 的 `Color::Rgb` 等于 `设计规范/01` 表对应值(如 Midnight `accent.primary == Rgb(0xb1,0x8c,0xf0)`、Daylight `bg.base == Rgb(0xf4,0xf1,0xea)`),任一漂移使单测失败

### Requirement: themed 渲染

`render` SHALL 接受 `&Theme` 参数,各组件按语义 token 上色(替代 cut1/cut2a 的硬编码 ANSI 色):品牌 / 占位用 `text.muted`,prompt marker / tag / 工具名用 `accent.primary`,权限框用 `warning.fg`/`warning.bg`,工具卡 `✓` 用 `success.fg`、`✗` 用 `error.fg`,状态行 phase 按 `设计规范/02` 状态机配色。run_tui MUST 默认 `Theme::midnight()`。既有四区 / 工具卡 / phase 的**结构**不变。

#### Scenario: 同结构两主题异色

- **WHEN** 以 `Theme::midnight()` 与 `Theme::daylight()` 分别渲染同一 `AppState`
- **THEN** 两帧**文本结构一致**、**配色按各自调色板不同**(经带色快照可分辨)

### Requirement: 带色快照锁定(token 名)

系统 SHALL 提供带色快照表示 `buffer_to_styled(buffer, &Theme)`:在文本基础上,把每 cell 的 `fg`/`bg`(及关键 `Modifier`)**反查映射为语义 token 名**并注入快照,使 token **赋值错误**(用错 token)经快照 diff 暴露;token **值漂移**由 token 单测覆盖。既有 text-only 快照 MUST 迁移为带色表示(superset:文本 + 色注解)。

#### Scenario: 配色赋错被快照拦截

- **WHEN** 渲染产物里某区域的 token 赋值改变(如工具名从 `accent.primary` 误改为 `error.fg`)
- **THEN** 该区域的带色快照与锁定值不一致,测试失败(纯文本快照无法察觉此变化)

#### Scenario: welcome 两主题带色快照

- **WHEN** 以 Midnight 与 Daylight 渲染 welcome 态并 `buffer_to_styled`
- **THEN** 各得带色快照(文本 + token 名注解),与锁定一致;首帧经人工对 `原型截图/midnight-01-欢迎态` 与 `daylight-01-欢迎态` 审核后锁定

### Requirement: C6 权限框 diff body(args 派生)

权限框 SHALL 在头(`▲ 需要授权` + 工具名 + args)下渲染**从 `args` 派生**的 diff body(`设计规范/03` C6),不读文件:`write_file` 的 `content` 整段作 add 行;`edit_file` 的 `old_string` 作 del 行 + `new_string` 作 add 行;`run_shell` 显示命令、无 diff。diff 行色 add=`success.fg`(`+` gutter)/ del=`error.fg`(`−` gutter)/ ctx=`text.body`。动作行 `[y · 允许]` / `[n · 拒绝]`,其中 `[n · 拒绝]` MUST 用 `error.fg`(`设计规范/01`「拒绝=error.fg」)。diff 计算 MUST 为可单测的纯函数(`args` → diff 行),不触文件系统。

#### Scenario: write / edit / shell 的 diff 派生(纯函数)

- **WHEN** 对 `write_file{content}` / `edit_file{old_string,new_string}` / `run_shell{command}` 计算 diff
- **THEN** 分别得到 全 add 行 / (del 行 + add 行) / 无 diff(仅命令),不读取任何文件

#### Scenario: 权限框带 diff 的带色快照

- **WHEN** 一个 `edit_file` 的 pending 权限态渲染到 `TestBackend`
- **THEN** 带色快照含 diff body(del=`error.fg` / add=`success.fg`)与动作行 `[n·拒绝]`=`error.fg`,与锁定一致

### Requirement: C7 致命错误框

`render` SHALL 把 `TranscriptBlock::Error`(由 `AgentEvent::Error` 落入,§9 致命路径)渲为致命错误框(`设计规范/03` C7):`error.bg` 底、`error.border` 描边、`error.fg` 文,含标致命的 title(Loop 已终止、不重试)。

#### Scenario: 致命错误框带色快照

- **WHEN** transcript 含一条 `Error(message)` 时渲染
- **THEN** 带色快照含 C7 致命框(error.bg/border/fg + title + message),与锁定一致

### Requirement: transcript 滚动

`AppState` SHALL 维护 transcript 的 `scroll_offset`:默认**跟随底部**(新内容自动到底);手动滚动支持 **PageUp / PageDown**(整页)、**`↑` / `↓`(行级步进,复用 `scroll_up` / `scroll_down`,每次 1 行)**、**`Home`(`scroll_to_top`,到顶)**、**`End`(`scroll_to_bottom`,回底并恢复底部跟随)**;**鼠标滚轮**(`MouseEventKind::ScrollUp` / `ScrollDown`)经行级步进滚动(默认每次 N 行)。`scroll_to_top` MUST 置 `scroll_offset = 0` 且 `follows_bottom = false`;`scroll_to_bottom` MUST 置 `follows_bottom = true`(下一帧贴底)。滚到非底部时新内容 MUST NOT 强制拉回底部(保持阅读位置);滚回底部时 MUST 恢复跟随;offset MUST clamp 在 [顶, 底](不越界)。**仅 transcript 滚动**,顶栏 / 状态行 / 输入框 / 权限框固定。所有滚动键处理 SHALL 仅响应 `KeyEventKind::Press`(忽略 Release / Repeat)。鼠标滚轮要求终端 guard 进入时启用、退出 / panic 时关闭鼠标捕获。offset / 跟随逻辑 MUST 可单测。

#### Scenario: 跟随、手动滚、clamp(逻辑可测)

- **WHEN** 在底部时追加新内容 → 仍贴底;PageUp 后追加新内容 → 保持当前位置(不回底);PageUp/PageDown 至边界 → offset clamp 不越顶 / 底
- **THEN** `scroll_offset` 按上述规则变化(纯逻辑断言)

#### Scenario: 行级 / 鼠标滚轮步进与触底恢复跟随(逻辑可测)

- **WHEN** 调 `scroll_up`(行级)上滚若干行,再 `scroll_down` 步进直至触底
- **THEN** 上滚后 `follows_bottom` 为假且 offset 按行级步进变化;触底后 `follows_bottom` 恢复为真(后续新内容再次贴底)

#### Scenario: Home 到顶 / End 回底恢复跟随(逻辑可测)

- **WHEN** 在跟随底部态调 `scroll_to_top`,随后调 `scroll_to_bottom`
- **THEN** `scroll_to_top` 后 `scroll_offset == 0` 且 `follows_bottom == false`(`visible_scroll_offset` 指向顶);`scroll_to_bottom` 后 `follows_bottom == true` 且 `visible_scroll_offset` 回到底部偏移

#### Scenario: 滚动后的 transcript 快照

- **WHEN** transcript 行数超视口且 `scroll_offset` 指向中段时渲染
- **THEN** 快照只显对应窗口的 transcript 行,顶栏 / 状态行 / 输入框位置不变

### Requirement: spinner 动画(确定性渲染)

running 工具卡与 `CallingModel` / `ExecutingTool` phase SHALL 显示动画 spinner(帧序列,如 braille,终端不支持则 ASCII fallback),替代静态字符。`render` MUST 仅依据 `AppState.spinner_frame`(当前帧 index)绘制 —— 即给定 state 渲染确定(insta 可锁固定帧);帧推进 `advance_spinner`(index 循环)MUST 为可单测纯逻辑;动画 tick(`run_tui` 的 `interval`)MUST 与 render / 逻辑解耦(不把时间引入 `render` / `AppState`)。Idle / done / error / WaitingForPermission 用静态 glyph。

#### Scenario: 帧推进循环(纯逻辑)

- **WHEN** 连续调用 `advance_spinner` N 次(N = 帧数)
- **THEN** `spinner_frame` 依次 `0→1→…→N-1→0` 循环

#### Scenario: 固定帧确定性快照

- **WHEN** 以某固定 `spinner_frame` 渲染一个 running 工具卡 / busy phase
- **THEN** 快照取该帧对应 spinner 字符,确定可锁(不依赖时间)

### Requirement: 命令块渲染(C8 / C9 / notice)

`render` SHALL 渲染命令产出的 transcript 块(`设计规范/03`):C8 帮助块(两列 `cmd` + `desc`,7 命令)、C9 快照块(`provider · model · iter X/maxIter · N msgs · cwd · tools: 7`)、notice 块(info / 占位提示,`info.fg` / 框)。带色,复用 `Theme` + `buffer_to_styled`。

#### Scenario: 帮助块与快照块带色快照

- **WHEN** transcript 含一个 C8 帮助块 / 一个 C9 快照块时渲染
- **THEN** 各自 `insta` 带色快照与锁定一致(C8 两列对齐 7 命令;C9 含 provider/model/iter/msgs/cwd/tools 字段)

### Requirement: 状态行常驻 meta

状态行右侧 SHALL 常驻显示 `provider · model · iter X/maxIter · N msgs · cwd`(`设计规范/02` C10),与左侧 phase 并存。`iter` 由 UI 统计当前轮的 `StatusChanged(CallingModel)` 次数得到(新轮 / `TurnComplete` 重置),`msgs` = `transcript` 中**对话块(`User` / `Assistant`)**数 —— 自「单一时间线」合并后,`Tool` 块与命令产出块(Help / Status / Notice)**不计入** `msgs`,保持「消息数」语义;其余取 session 快照(`/model` 切换后 model 同步更新)。

#### Scenario: 状态行 meta 快照

- **WHEN** 给定 session 快照(provider/model/maxIter/cwd)与若干 transcript 块(含 `Tool` 块)渲染
- **THEN** 状态行右侧带色快照含 `provider · model · iter X/maxIter · N msgs · cwd`,其中 `msgs` 只计 `User` / `Assistant` 块(不含 `Tool`),与锁定一致

### Requirement: 工具卡 C5 exit foot

`ToolCard` SHALL 携 `exit: Option<i32>`(由 `ToolCallFinished` 的 `outcome.exit` 填);`render` SHALL 仅在 `exit` 为 `Some` 时渲染 C5 脚 `exit {code}`(非 0 用 `error.fg`),`None` 时**不渲染 foot**(保证既有非进程类工具卡快照零回归)。

#### Scenario: run_shell 卡含 exit foot,其余不含

- **WHEN** 渲染一个 `exit = Some(0)` 的工具卡与一个 `exit = None` 的工具卡
- **THEN** 前者带色快照含 `exit 0` 脚;后者无 foot(与既有工具卡快照一致)

### Requirement: UserInput::SetModel 变体

`tui` 的 `UserInput` SHALL 增 `SetModel(String)` 变体;`run_agent_task` 收到时 MUST 对 idle 的 agent 调 `Agent::set_model`,不影响进行中的轮。

#### Scenario: SetModel 改后续轮 model

- **WHEN** agent-task 在两轮之间收到 `UserInput::SetModel("m2")`
- **THEN** 后续 `Prompt` 轮的 `ModelRequest.model` 为 `"m2"`

### Requirement: 运行中可中断(Esc 中断本轮)

`tui` 的 `UserInput` SHALL 增 `Interrupt` 变体、`AgentEvent` SHALL 增 `Interrupted` 变体。`run_agent_task` 的 `Prompt` 分支 MUST 以 `tokio::select!` 把本轮 `Agent::run_observed` 与一个**独立**中断信号并置:中断信号 MUST NOT 复用 `input_rx`(以免误吞排队的 `Prompt` / `SetModel`)。中断到达即 drop 本轮 run future(在 `provider.complete` / `tool.execute` 的 await 点协作取消),向 UI 发 `AgentEvent::Interrupted`、状态回 `Idle`,**agent task 与程序均存活**;中断后 MUST NOT 再次调用 provider。UI 端 Esc 按三态分流(仅响应 `KeyEventKind::Press`):`pending_permission` 存在 → 拒绝授权(不变);无 pending 且本轮运行中(phase 非 Idle/Ready)→ 投 `UserInput::Interrupt`(中断);无 pending 且就绪 → 退出程序(不变)。

#### Scenario: 运行中中断以 Interrupted 收场且不再调用 provider

- **WHEN** 以 Mock provider(在 `complete` 中挂起的脚本)驱动 `run_agent_task`,投入 `Prompt` 后再投 `Interrupt`
- **THEN** 本轮以 `AgentEvent::Interrupted` 收场、状态回 `Idle`,provider 不被再次调用,agent task 继续存活待下一个 `UserInput`(全程无终端)

#### Scenario: 中断不消费排队的 Prompt

- **WHEN** 中断信号经独立通道到达,而 `input_rx` 中另有排队的 `Prompt`
- **THEN** 仅本轮被中断;排队的 `Prompt` 不被中断臂吞掉,后续仍可正常消费并跑完

#### Scenario: Esc 三态分流

- **WHEN** 分别在「pending 授权」/「本轮运行中、无 pending」/「就绪、无 pending」三态下收到 Esc(`KeyEventKind::Press`)
- **THEN** 依次为:经 oneshot 回送 `Deny` 拒绝授权 / 投出 `UserInput::Interrupt` / `should_exit` 为真(退出);三态互斥

#### Scenario: 中断态渲染为非致命 notice

- **WHEN** UI 收到 `AgentEvent::Interrupted` 后渲染
- **THEN** transcript 末尾含一条「⊘ 已中断本轮」notice 块(`info.fg` / 非致命,区别于 C7 致命错误框),与锁定带色快照一致

### Requirement: 单一时间线 transcript

`TranscriptBlock` SHALL 增 `Tool(ToolCard)` 变体;`AppState` MUST 删除独立的 `tool_cards` Vec,改为把工具卡按**到达顺序**并入唯一的 `transcript` Vec:`ToolCallStarted` 时 push 一个 `running` 态 `Tool(ToolCard)` 块;`ToolCallFinished` 时按 `id` 在 `transcript` 中**回填**对应卡(done/error + output + exit)。`render` MUST 顺序遍历 `transcript` 渲染各块(文本块按文本、`Tool` 块按 `设计规范/03` C5),**不再**在末尾汇总工具卡。如此自然收尾后最终回答为 `transcript` 末块,钉底即可见。`ToolCallFinished` 未匹配到 `id`(异常)时 MUST 安全降级(忽略),不得 panic。

#### Scenario: 工具卡按到达顺序入时间线、Finished 回填

- **WHEN** 依次 `apply(ToolCallStarted{id,..})` 与 `apply(ToolCallFinished{id, outcome})`
- **THEN** `transcript` 在到达位置含一个 `Tool` 块,Started 后为 `running`、Finished 后回填为 done/error 且带 output / exit;不存在独立的 `tool_cards` Vec

#### Scenario: 最终回答在工具卡之后可见(快照)

- **WHEN** 一条 transcript 含「`User` → `Tool`(done) → `Assistant`(最终回答)」顺序时渲染
- **THEN** 带色快照中 `Assistant` 最终回答块位于 `Tool` 卡**之后**(为末块、不被工具卡盖住),与锁定快照一致

#### Scenario: Finished 无匹配 id 安全降级

- **WHEN** `apply(ToolCallFinished{id,..})` 而 `transcript` 中无该 `id` 的 `Tool` 块
- **THEN** 状态不变、不 panic

### Requirement: 终端文本排版与宽度度量

`render` SHALL 提供按显示宽度的 transcript 文本排版:① `User` / `Assistant` 文本块按视口宽度**换行**,续行**悬挂缩进**对齐 marker 宽度;② `Assistant` marker 为 `◆ `、`User` marker 为 `> `;③ 宽度度量 `display_width` MUST 把 CJK / 全角与常见 emoji 记为 2 列、组合 / 零宽字符记为 0 列;④ 输入框 MUST 把光标定位到输入串末尾(按显示宽度);⑤ C2 欢迎态(`设计规范/03`)文本**水平居中**,空会话时整体**垂直留白**居中。以上 MUST 可经 `TestBackend` 渲染断言 / `insta` 快照验证。

#### Scenario: 显示宽度度量

- **WHEN** 计算 `display_width("a")` / `display_width("你好")` / `display_width("👋")`
- **THEN** 分别为 `1` / `4` / `2`

#### Scenario: Assistant 块换行 + 悬挂缩进 + 宽度

- **WHEN** 一个超视口宽度、含 emoji 的 `Assistant` 块渲染到窄 `TestBackend`
- **THEN** 首行以 `◆ ` 起、续行按 marker 宽度悬挂缩进对齐,文本按显示宽度换行(不串列、不撑破视口)

#### Scenario: 输入框光标定位末尾

- **WHEN** 输入串为 `你好` 时渲染
- **THEN** 终端光标位于 `prompt + 输入串显示宽度` 之后的列(`set_cursor_position` 命中)

#### Scenario: 欢迎态居中(快照)

- **WHEN** 空会话渲染到 `TestBackend`
- **THEN** 带色快照中 C2 欢迎态文本水平居中、整体垂直留白居中,与锁定快照一致

### Requirement: 按键事件去重(仅 Press)

TUI 按键处理 MUST 仅响应 `KeyEventKind::Press`:`on_key` / `should_exit` / 滚动键处理 SHALL 忽略 `Release` / `Repeat` 事件,避免 Windows 终端每次按键三发(Press/Repeat/Release)导致的重复输入与误触发。

#### Scenario: 非 Press 事件被忽略

- **WHEN** 对同一字符依次投入 `Press` / `Release` / `Repeat` 三个 `KeyEvent`
- **THEN** 仅 `Press` 生效(输入串只增一个该字符);`Esc` 的 `Release` / `Repeat` 不触发退出

### Requirement: 工具输出折叠与全局展开(ctrl+o)

`AppState` SHALL 持折叠态 `tools_expanded: bool`(默认 `false` = 折叠)。`render` MUST 据 `tools_expanded` 对每个 `TranscriptBlock::Tool(ToolCard)` 块二选一渲染:**折叠**态仅渲染 `设计规范/03` C5 的**单行头**(状态 glyph + 工具名 + args 摘要 + 结果摘要),**不**渲染 output 体与脚,且单行头 **MUST NOT 含** `┌─` 边框前缀(`┌─` / `└─` 边框仅展开态用);**展开**态渲染**全量**(头 / 体 / 脚 + 截断标记,即现状)。折叠行结果摘要 SHALL 为:`done` 且 output 非空 → ` · {N} 行 ⌄`(N = output 行数,`⌄` 提示可展开);携 `exit` → ` · exit {code}`(非 0 用 `error.fg`);`running` → ` · 运行中…`。

连续的 `Tool` 块 SHALL 视为一组:**组内相邻 `Tool` 块之间 MUST NOT 插入空行**(紧凑呈现);组边界(相邻块非 `Tool`,或位于 transcript 端点)仍插入空行分隔。折叠态下,每个连续 `Tool` 组的**组首**卡片 SHALL 在结果摘要后追加 ` · ctrl+o 展开`(`text.muted`);**组内非组首**卡片与**展开**态 MUST NOT 追加该提示(每组仅一次、补展开可发现性)。

`ctrl+o`(`KeyCode::Char('o')` + `KeyModifiers::CONTROL`,**仅** `KeyEventKind::Press`)MUST 翻转 `tools_expanded`(全局展开/折叠**所有**工具卡),且 MUST NOT 把 `o` 写入输入框(在文本输入 arm 之前拦截)。折叠**仅作用于** `Tool` 块;`User` / `Assistant` / `Error` / `Help` / `Status` / `Notice` 块 MUST NOT 受折叠影响。本期**只**提供全局 toggle,不提供单条卡片独立展开。

#### Scenario: 工具卡默认折叠为单行(带色快照)

- **WHEN** 一个 `done` 且 output 多行的 `Tool` 块(transcript 中唯一,即所在组组首)在 `tools_expanded == false`(默认)下渲染到 `TestBackend`
- **THEN** 带色快照仅含该卡的单行头(glyph + 工具名 + args + ` · {N} 行 ⌄` + ` · ctrl+o 展开`),**不**含 `┌─` 头边框、output 体行与 `└─` 脚,与锁定一致

#### Scenario: ctrl+o 全局展开再折回(逻辑 + 快照)

- **WHEN** 对含若干 `Tool` 块的 `AppState` 投入 `ctrl+o`(`Char('o')`+`CONTROL`,`Press`)一次,再投一次
- **THEN** 第一次后 `tools_expanded == true` 且所有 `Tool` 块渲为全量(头 + 体 + 脚 + 截断标记);第二次后 `tools_expanded == false` 折回单行;两态带色快照各与锁定一致

#### Scenario: ctrl+o 仅 Press 且不写入输入框

- **WHEN** 依次投入 `ctrl+o` 的 `Release` 与 `Repeat`,再投入 `Press`
- **THEN** 仅 `Press` 翻转 `tools_expanded`;`Release` / `Repeat` 不翻转;任一情况下输入框串均不出现字符 `o`

#### Scenario: 折叠仅作用于 Tool 块

- **WHEN** transcript 含 `User` / `Assistant` / `Tool`(done)三块且 `tools_expanded == false` 时渲染
- **THEN** `User` / `Assistant` 块仍**全文**渲染(不折叠),仅 `Tool` 块折为单行,与锁定带色快照一致

#### Scenario: 连续工具卡分组紧凑且仅组首带展开提示(带色快照)

- **WHEN** transcript 为 `User` → `Tool`(read,done)→ `Assistant` → `Tool`(write,done)→ `Tool`(grep,done),在 `tools_expanded == false` 下渲染
- **THEN** read 与 write 各为所在组组首、行尾带 ` · ctrl+o 展开`;grep 为 write 组的组内非首、**不**带该提示;write 与 grep 之间**无**空行(同组紧凑);与锁定带色快照 `tui_tool_group_ctrl_o_hints` 一致

### Requirement: 键盘滚动全覆盖与鼠标滚轮降级(ConPTY 限制)

键盘 SHALL 提供 transcript 的**完整**导航且**不依赖**鼠标捕获:行级(`↑` / `↓`)、整页(`PageUp` / `PageDown`)、到顶(`Home`)、回底并恢复跟随(`End`)合起来 MUST 能从任意位置到达 transcript 的**顶**与**底**。鼠标滚轮(`MouseEventKind::ScrollUp` / `ScrollDown`)SHALL 作为**尽力而为**的增强:在转发滚轮事件的终端(多数 Unix 终端、较新 Windows 11 conhost)可用;在 **ConPTY 不转发滚轮的 Windows 构建**(平台限制,见 design.md 决策 ③ 与 `microsoft/terminal` #376 / #545)上滚轮事件不到达 crossterm,此失效 MUST NOT 削弱键盘全覆盖(滚轮缺失时仍可纯键盘遍历全文)。行级滚动键与鼠标滚轮 MUST 复用同一 `scroll_up` / `scroll_down` 原语,使键盘可达滚轮所能达的任意滚动位置(键盘为滚轮能力的超集)。`terminal.rs` 的鼠标捕获 MUST 保持开启(失效根因在平台而非捕获缺失,不因本 change 关闭捕获)。

#### Scenario: 纯键盘到顶与回底(无任何 MouseEvent)

- **WHEN** transcript 行数超视口、处于跟随底部态,**不**投入任何 `Event::Mouse`,仅以键盘调用 `scroll_to_top` 再 `scroll_to_bottom`
- **THEN** `scroll_to_top` 后 `visible_scroll_offset` 指向顶(0)、`follows_bottom` 为假;`scroll_to_bottom` 后 `follows_bottom` 为真且 `visible_scroll_offset` 回到底部偏移

#### Scenario: 滚轮缺失无能力损失(行级键 == 滚轮)

- **WHEN** 以键盘 `↑`(行级,`scroll_up` 1 行)上滚 K 次,与「等量鼠标滚轮上滚」对照
- **THEN** 二者皆经同一 `scroll_up` 原语改变 `scroll_offset`,到达相同偏移;即使滚轮事件永不到达,键盘行级步进仍可复现任意滚动位置

### Requirement: 诊断事件日志(env 门控)

TUI SHALL 提供环境变量门控的事件诊断日志:当 `MYSTERIES_TUI_DEBUG_EVENTS` 被设置且非空时,`run_tui` SHOULD 将 crossterm `Event` 的脱敏摘要追加写入 `std::env::temp_dir()` 下固定文件名(如 `mysteries-tui-events.log`),用于真机核验滚轮事件是否到达。日志写入失败 MUST 静默降级,不得中断主循环或改变 TUI 交互语义。核心格式化函数 `debug_event_line(&Event) -> String` MUST 是纯函数、可单测、输出确定。诊断日志 MUST NOT 记录凭据、prompt 正文、配置路径、cwd 或其它用户文件内容;`KeyCode::Char` 的具体字符 MUST 脱敏,只保留事件类别 / key kind / modifiers / mouse kind 等定位滚轮所需元数据。

#### Scenario: 已知 Event 生成确定诊断行

- **WHEN** 调用 `debug_event_line(&Event::Mouse(MouseEventKind::ScrollUp, ...))` 与 `debug_event_line(&Event::Key(KeyCode::Char('x') + CONTROL, Press))`
- **THEN** 输出行确定且包含事件类别、kind / modifiers 等结构信息;Key 行不包含字符 `x`,避免把用户输入正文写入日志

### Requirement: transcript 视口渲染保真(可见行数对齐视口高度)

`render` 渲染 transcript 时,实际占用的**屏幕行数 MUST 等于 `visible_transcript_lines` 切出的逻辑行数**且 MUST ≤ 视口高度。`visible_transcript_lines` 已按**预换行后**的行数精确切出 `viewport_lines` 行(`skip(offset).take(viewport_lines)`),故渲染 MUST NOT 依赖 `Paragraph` 的二次换行:`render_transcript` MUST NOT 对 transcript `Paragraph` 施加 `.wrap`。所有需要换行才完整可读的 transcript 行(`User` / `Assistant` 文本、展开态工具卡 `output` 体)MUST 在进入切片**前**按显示宽度 ≤ 视口宽度**预换行**;装饰边框(工具卡脚 / `Error`/`Help`/`Status` 的 `┌─…`/`└──…`)MUST 按渲染 `width` **自适配**生成,使每边框行显示宽度 ≤ 视口宽度、占恰好 1 个屏幕行(不二次换行、不增加屏幕行数);整行工具卡头等无法预换行的固定宽装饰在更窄终端 MAY 被**右端截断**。当 `follows_bottom` 为真时,transcript 的**最新(底部)内容 MUST 在视口内可见**,MUST NOT 因二次换行溢出被裁到视口下方。本不变量 MUST 可经 `ratatui::backend::TestBackend`(窄宽 + 超视口多块内容,含会触发二次换行的行 / 长工具输出)断言。

#### Scenario: 超视口内容跟随底部时最新内容可见

- **WHEN** 在窄 `TestBackend`(宽 < 80)上渲染一个预换行后总行数超视口、且含会触发 `Paragraph` 二次换行的行(如 80 宽边框块 / 超宽长工具输出)的 transcript,`follows_bottom` 为真,末块为含可识别串的最新 `User` / `Assistant` 内容
- **THEN** 渲染输出**包含**该末块(最新内容)的可识别串(底部不被裁),且 transcript 区实际屏幕行数不超过视口高度

#### Scenario: 边框按 width 自适配占 1 屏幕行不顶高

- **WHEN** 在宽 < 80 的 `TestBackend` 上渲染一个含装饰边框的块(如 `Error` 致命错误框)
- **THEN** 边框行按渲染 `width` 自适配生成,占据**恰好 1 个屏幕行**,MUST NOT 被二次换行成 2 行而把后续(更新)内容向下挤出视口

#### Scenario: 展开态工具输出长行预换行

- **WHEN** `tools_expanded` 为真,一个 `Tool` 卡的 `output` 含显示宽度超视口宽度的长行,渲染到窄 `TestBackend`
- **THEN** 该长行在进入切片**前**已按 ≤ 视口宽度预换行为多个逻辑行(内容不被整行截断丢失),且 transcript 区实际屏幕行数仍等于切出的逻辑行数(不依赖 `Paragraph` 二次换行)

