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

`AppState` SHALL 维护 transcript 的 `scroll_offset`:默认**跟随底部**(新内容自动到底);手动 **PageUp / PageDown** 滚动;滚到非底部时新内容 MUST NOT 强制拉回底部(保持阅读位置);offset MUST clamp 在 [顶, 底](不越界)。**仅 transcript 滚动**,顶栏 / 状态行 / 输入框 / 权限框固定。offset 逻辑 MUST 可单测。

#### Scenario: 跟随、手动滚、clamp(逻辑可测)

- **WHEN** 在底部时追加新内容 → 仍贴底;PageUp 后追加新内容 → 保持当前位置(不回底);PageUp/PageDown 至边界 → offset clamp 不越顶 / 底
- **THEN** `scroll_offset` 按上述规则变化(纯逻辑断言)

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

状态行右侧 SHALL 常驻显示 `provider · model · iter X/maxIter · N msgs · cwd`(`设计规范/02` C10),与左侧 phase 并存。`iter` 由 UI 统计当前轮的 `StatusChanged(CallingModel)` 次数得到(新轮 / `TurnComplete` 重置),`msgs` = transcript 块数,其余取 session 快照(`/model` 切换后 model 同步更新)。

#### Scenario: 状态行 meta 快照

- **WHEN** 给定 session 快照(provider/model/maxIter/cwd)与若干 transcript 块渲染
- **THEN** 状态行右侧带色快照含 `provider · model · iter X/maxIter · N msgs · cwd`,与锁定一致

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

