# tui-shell Specification

## Purpose
定义 mysteries 交互式 TUI 外壳的全部交互契约,覆盖三层:架构上以双-task + channel 运行,UI 与 agent task 经 `UserInput` / `AgentEvent` 通信,经既有 `DeltaSink` / `PermissionDecider` / `AgentObserver` 缝接入内核、不改 agent-loop;呈现上约定四区布局、主题令牌与 transcript 渲染(工具卡、markdown、滚动与选区);交互上约定输入到会话的完整行为(多行编辑、粘贴合并与折叠、输入历史、命令补全、模型 picker、权限授权与模式、消息排队、中断)及终端生命周期(raw mode / 鼠标捕获的进入与恢复)。关键立场是逻辑与呈现分层:状态与按键归约为可单测纯函数(时间与 IO 不进入 `AppState` / `render`),渲染对给定状态确定,以 `TestBackend` + `insta` 带色快照做事后回归。命令语义属 builtin-commands,Agent 循环 / 工具 / 权限判定属 headless 内核各域;本域覆盖它们在终端上的全部呈现与交互编排。
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

`ChannelDecider` SHALL impl 既有 async `PermissionDecider`:`decide` MUST 先查注入的 `PolicyEngine`(见 `permission-gate` 的「命令 allowlist 自动放行」)——调用的 permission key 命中 allowlist 即返回 `Allow`、**不发起 channel 往返**;未命中且 `auto_allows(mode, level)` 未命中时,MUST 创建 `oneshot`、向 UI 发 `AgentEvent::PermissionRequired{tool_name, args, allow_always_key, responder}`(`allow_always_key: Option<String>` 为该调用的 permission key,`Some` 表示可 always-allow),在 `responder` 的 `rx.await` 处挂起,收到 `PermissionReply` 后映射为 `PermissionDecision` 返回:`AllowOnce → Allow`;`AllowAlways →` 记忆 + 持久化(见 `permission-gate` 的「always-allow 记忆与持久化」)`→ Allow`;`Deny`、或 UI 端 sender / responder 断开,MUST 返回 `PermissionDecision::Deny`(fail-safe)。`decide` 返回类型仍为既有 `PermissionDecision {Allow, Deny}`。本机制 MUST 不改动 `agent-loop`(经既有 `PermissionDecider` 缝接入)。

#### Scenario: 权限请求挂起-恢复

- **WHEN** `ChannelDecider::decide` 被调用(allowlist 与 `auto_allows` 均未命中),UI 收到 `PermissionRequired` 后经 `responder` 回送 `AllowOnce`
- **THEN** `decide` 返回 `Allow`(挂起在 `rx.await`、收到后恢复)

#### Scenario: UI 断开 fail-safe 拒绝

- **WHEN** `decide` 发出请求后 UI 端 responder 被丢弃(`rx` 出错)
- **THEN** `decide` 返回 `Deny`,不 panic

#### Scenario: allowlist 命中不发起 channel 往返

- **WHEN** 调用的 permission key 已在注入 `PolicyEngine` 的 allowlist 中
- **THEN** `decide` 直接返回 `Allow`,不创建 `oneshot`、不发 `PermissionRequired`

### Requirement: agent-task 一轮编排(Mock 驱动 · 无终端)

系统 SHALL 提供可在**无终端**下以 Mock provider 驱动的 agent-task 编排:投入一个 prompt,经 `ChannelSink`(文本)与 `ChannelDecider`(权限)跑完一轮 `Agent.run`,把事件流回 channel。含非 `ReadOnly`(`Edit` / `Execute`)工具的脚本 MUST 能走通「`PermissionRequired` → 回送决策 → 继续 / 拒绝入 history」。

#### Scenario: 含权限的一轮编排

- **WHEN** Mock 脚本为「轮1 → 一个非 `ReadOnly`(`Edit` / `Execute`)工具的 tool_call、轮2 → 终复文本」,投入 prompt 并对 `PermissionRequired` 回送 `Allow`
- **THEN** channel 依次见到权限请求与文本事件,工具被执行,最终 `TurnComplete`;全程无终端、不触网

### Requirement: ratatui 四区最小外壳渲染

系统 SHALL 用 ratatui 渲染 `设计规范/02-布局与交互` 的四区布局,自上而下:顶栏(C1,仅品牌 `✦ mysteries  agent · v1.0`)/ transcript(空会话 → C2 欢迎态;有会话 → user/assistant 文本块)/ **输入框(C11,`mysteries ▸ ` + 占位)/ 状态行(C10,cut1 粗 phase:就绪 / 忙 / 等待授权)**——**状态行位于最底、输入框在其上方**(贴 claude code 底部状态栏;adapt 设计规范 02 原型「状态行在输入框上方」)。`PermissionRequired` pending 时,C6 权限框 MUST 内联钉在**输入框上方**。渲染 MUST 可经 `ratatui::backend::TestBackend` 快照验证(`insta`)。

#### Scenario: 欢迎态结构快照

- **WHEN** 空会话状态渲染到 `TestBackend`
- **THEN** 快照自上而下含 顶栏品牌行、C2 欢迎态、输入框占位、**最底状态行**四区结构,且与锁定的 `insta` 快照一致

#### Scenario: 权限态内联框

- **WHEN** 存在一个 pending 的 `PermissionRequired`(工具名 + args)时渲染
- **THEN** 快照在**输入框上方**含 C6 权限框(`▲ 需要授权` + 工具名/args + `[y·允许][n·拒绝]`),与锁定快照一致

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

`AppState` SHALL 据 `ToolCallStarted` / `ToolCallFinished` 维护工具卡块;`render` SHALL 按 `设计规范/03` C5 渲染:头(状态 glyph `running`→spinner 动画帧 / `done`→`✓` / `error`→`✗` + 工具名 + args;只读工具带 `只读 · 自动运行` 徽章)、体(`output` 行;截断时 `⋯ +N 行已截断`)、脚(`exit {code}`)。着色契约见「themed 渲染」requirement,running 态 spinner 见「spinner 动画(确定性渲染)」requirement。

**diff 体(write_file / edit_file)**:write_file / edit_file 卡 SHALL 渲染着色 diff 体,**与 `tools_expanded` 解耦、恒显**(分型策略:diff 是高价值内容默认露出,输出噪音默认收起)——展开态位于头行与 output 行之间;**折叠态位于单行头之后**(仍不渲 output 体 / 脚 / `┌─`/`└─` 边框,diff 行保留 `│ ` 前缀)。数据源为 `compute_diff(card.name, card.args)`(args 纯推导,MUST NOT 读文件):`Del` 行 `− ` 前缀 `error.fg`、`Add` 行 `+ ` 前缀 `success.fg`,各行带 `│ ` 边框前缀(`border_subtle`)、底色 `bg.base`(权限确认框的 warning_bg diff 风格与不设上限策略**不变**)。(实现 MAY 防御性处理 `Ctx` → `text.body`;`compute_diff` 现不产 `Ctx`,不在契约面。)

**宽度与折行**:diff 行内容宽 MUST 为 `width.saturating_sub(4).max(1)`(整行宽 − `│ ` 2 列 − 标记 2 列;区别于 output 行的 −2),按既有 wrap 以显示宽度折行;续行以 `│ ` + 两空格占位起头(恰补标记列,首行续行内容同宽)、同色、MUST NOT 重复 `+ `/`− ` 标记;任一屏行显示宽 MUST NOT 超视口宽(不得溢出被截,保选区复制完整)。

**截断(按屏行,双配额)**:diff 体 SHALL 按**屏行预算**截断(按折行后的显示行计,允许止于某条 `DiffLine` 的折行中途):**展开态** `DIFF_MAX_ROWS`(= 24)、**折叠态** `DIFF_COLLAPSED_MAX_ROWS`(= 8),均具名常量;超预算时止于预算并渲尾行 `⋯ 其余 N 行`(N = 未被**完整**显示的 `DiffLine` 数,`text.muted`,带 `│ ` 边框前缀)。

**头行 args**:这两个工具的展开态头行 args 亦 SHALL 用既有 `tool_args_preview`(`path=...`;preview 缺 `path` 时沿用既有整段 JSON fallback),除该 fallback 外 MUST NOT 渲整段 JSON。diff 体(折叠与展开)与展开态头行 preview SHALL **不分态**渲染(`Running` / `Error` 亦同,呈现"请求的变更";折叠计数仍仅 `Done`,见下)。

**空 diff 与其他工具**:`compute_diff` 为空(diff 参数缺失或为空串——write 的 `content`;edit 的 `old_string` 与 `new_string` **均**缺失/空串)或非 write/edit 工具时,MUST 不渲染任何 diff 体行:非 write/edit 工具行集与既有完全一致;空 diff 的 write/edit 卡折叠态与既有一致、展开态除头行 args 改 preview 外其余行一致。transcript 行数核算与渲染 MUST 共用同一行集来源(不另设第二套计数)。

**折叠摘要计数**:`Done` 且 diff 非空的 write_file / edit_file,折叠行结果摘要 SHALL 为 ` · +A −D ⌄`(A / D = `Add` / `Del` 行数,`+A` 用 `success.fg`、`−D` 用 `error.fg`,`−` 与 diff 前缀同字符 U+2212,点缀 ` · ` 与 ` ⌄` 用 `text.secondary`,**为 0 的一侧省略**);判定优先级与属主契约见 MODIFIED「工具输出折叠与全局展开(ctrl+o)」。`Running` / `Error` 折叠摘要 MUST 维持既有形态(不显 +/− 计数,防误读为已应用)。

#### Scenario: 工具卡三态结构快照

- **WHEN** 分别以 running / done / error 态的工具卡渲染到 `TestBackend`
- **THEN** `insta` 快照含 C5 结构(glyph + 名 + args + 只读徽章 + output + exit + 截断标记),且与锁定快照一致

#### Scenario: edit_file 展开渲染着色 diff 体

- **WHEN** `tools_expanded`,`Done` 的 edit_file 卡(args 含 `path`、两行 `old_string`、两行 `new_string`,output 非空)渲染
- **THEN** 头行 args 为 `path=...` preview(非整段 JSON);头行与 output 行之间依次 2 条 `− `(`error.fg`)+ 2 条 `+ `(`success.fg`)行,各带 `│ ` 前缀(`border_subtle`)、`bg.base` 底;与锁定带色快照一致

#### Scenario: write_file content 全 Add

- **WHEN** `tools_expanded`,`Done` 的 write_file 卡(args 含 `path` 与多行 `content`,output 非空)渲染
- **THEN** diff 体为逐行 `+ `(`success.fg`),条数 = content 行数;与锁定带色快照一致

#### Scenario: 折叠摘要 +A −D 仅 Done、零侧双向省略

- **WHEN** 折叠态 `Done` 的 edit_file 卡(old 2 行 / new 3 行)、write_file 卡(content 12 行)、edit_file 卡(old 2 行 / `new_string` 空串);另以同 args 渲染 `Running` / `Error`(output 2 行、无 exit)态
- **THEN** 三张 Done 卡摘要分别含 ` · +3 −2 ⌄`(`+3` success.fg、`−2` error.fg)、` · +12 ⌄`(Del 侧省略)、` · −2 ⌄`(Add 侧省略);`Running` 仍 ` · 运行中…`、`Error` 仍 ` · 2 行 ⌄`,均不显 +/− 计数

#### Scenario: 超 DIFF_MAX_ROWS 截断(短行)

- **WHEN** `tools_expanded`,write_file 卡 content 为 30 条短行(各占 1 屏行)渲染
- **THEN** 恰渲 24 条 diff 屏行 + 尾行 `⋯ 其余 6 行`(`text.muted`,带 `│ ` 前缀);output 行照常在其后;transcript 行数核算含 diff 行与尾行

#### Scenario: 单条超长行按屏行截断(minified 场景)

- **WHEN** `tools_expanded`,write_file 卡 content 为**单条**超长行(如显示宽 1200,窄视口下折行后 > 24 屏行)渲染
- **THEN** diff 体恰 24 条屏行(止于该逻辑行折行中途)+ 尾行 `⋯ 其余 1 行`(该 `DiffLine` 未被完整显示,计 1)

#### Scenario: 超宽 diff 行折行(窄视口、含 CJK)

- **WHEN** `tools_expanded`,write_file 卡 content 含一条显示宽度超视口的长行(含 CJK 宽字符),渲染到窄 `TestBackend`(如 width = 40)
- **THEN** 该 `DiffLine` 折为多条屏行:首行以 `│ ` + `+ ` 起,续行以 `│ ` + 两空格占位起、同 `success.fg`、不重复 `+ `;各屏行显示宽 ≤ 视口宽(内容宽 = 整行宽 − 4);与锁定带色快照一致

#### Scenario: Running / Error 展开不分态渲染 diff 体

- **WHEN** `tools_expanded`,同一 edit_file args(含 `path`、old 2 行、new 2 行)分别以 `Running` / `Error`(output 非空)态渲染
- **THEN** 两态头行 args 均为 `path=...` preview;头行后均渲 2 条 `− ` + 2 条 `+ `(着色同 Done);`Running` 的体行为「运行中…」占位、`Error` 的体行为 output 文本;与锁定带色快照一致

#### Scenario: 折叠态 diff 体恒显与折叠配额截断

- **WHEN** 折叠(默认,`tools_expanded == false`)下渲染:`Done` 的 edit_file 卡(old 2 行 / new 3 行)、`Done` 的 write_file 卡(content 12 行);另以同 edit args 渲染 `Running` 态
- **THEN** edit 卡单行头(含 ` · +3 −2 ⌄`)之后渲 5 条 diff 行(2 `− ` + 3 `+ `,≤ 8 全显);write 卡单行头(含 ` · +12 ⌄`)之后渲 8 条 `+ ` + 尾行 `⋯ 其余 4 行`(折叠配额 8);`Running` 卡单行头(` · 运行中…`)之后亦渲 diff 体;三卡均**不**渲 output 体与 `┌─`/`└─` 边框;与锁定带色快照一致

#### Scenario: 空 diff 与非 diff 工具零回归

- **WHEN** run_shell / read_file 等非 write/edit 工具卡,以及**缺齐 diff 参数**(即 `compute_diff` 为空:write 缺 `content` 或 `content` 为空串;edit 的 `old_string`/`new_string` 均缺失)的 write/edit 卡,分别以折叠与展开渲染
- **THEN** 均不渲任何 diff 体行、不显 +/− 计数;非 write/edit 工具卡行集与既有锁定快照一致;空 diff 的 write/edit 卡折叠态与既有一致、展开态仅头行 args 为 `path=...` preview(既有快照仅 `tui_tool_card_expanded_done` 因此更新);其余既有快照与锁定一致

### Requirement: 全 phase 状态行 C10

**活动状态行**(输入框上方,见「活动状态行(输入框上方)」requirement)SHALL 据 `StatusChanged` 显示完整 phase(`设计规范/02` 状态机):`Idle`→`◇ 就绪`(idle 简显)、`CallingModel`→`调用模型…`、`ExecutingTool(name)`→`执行 {name}…`、`WaitingForPermission`→`▲ 等待授权…`。phase label MUST 渲染在**活动状态行**(输入框上方),MUST NOT 渲染在底部状态行。`AppState` 的 phase 状态 MUST 可单测,渲染 MUST 可 `insta` 快照。

#### Scenario: phase 随事件更新(状态可测)

- **WHEN** `AppState.apply(StatusChanged(ExecutingTool("write_file")))`
- **THEN** 其 phase 为 `ExecutingTool("write_file")`,后续渲染**活动状态行**显示 `执行 write_file…`(底部状态行不含 phase)

#### Scenario: 各 phase 活动状态行快照

- **WHEN** 分别以 `Idle` / `CallingModel` / `ExecutingTool(x)` / `WaitingForPermission` 渲染
- **THEN** 各自 `insta` 快照在**活动状态行**显示对应 glyph + label(正确),底部状态行均不含 phase,与锁定一致

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

`AppState` SHALL 维护 transcript 的 `scroll_offset`:默认**跟随底部**(新内容自动到底);手动滚动支持 **PageUp / PageDown**(整页)、**`Ctrl+Home`(`scroll_to_top`,到顶)**、**`Ctrl+End`(`scroll_to_bottom`,回底并恢复底部跟随)**;**鼠标滚轮**(`MouseEventKind::ScrollUp` / `ScrollDown`)经行级步进滚动(默认每次 N 行)。**`↑` / `↓` 与裸 `Home` / `End` 不再用于 transcript 滚动**——`↑↓` 归多行光标 / 输入历史(见「输入历史 ↑↓ 召回」)、裸 `Home` / `End` 归输入行内光标(见「多行输入编辑」)。`scroll_to_top` MUST 置 `scroll_offset = 0` 且 `follows_bottom = false`;`scroll_to_bottom` MUST 置 `follows_bottom = true`(下一帧贴底)。滚到非底部时新内容 MUST NOT 强制拉回底部;滚回底部时 MUST 恢复跟随;offset MUST clamp 在 [顶, 底]。**仅 transcript 滚动**,顶栏 / 状态行 / 输入框 / 权限框固定。所有滚动键处理 SHALL 仅响应 `KeyEventKind::Press`。鼠标滚轮要求终端 guard 进入时启用、退出 / panic 时关闭鼠标捕获。offset / 跟随逻辑 MUST 可单测。

#### Scenario: 跟随、手动滚、clamp(逻辑可测)

- **WHEN** 在底部时追加新内容 → 仍贴底;PageUp 后追加新内容 → 保持当前位置;PageUp/PageDown 至边界 → offset clamp 不越顶 / 底
- **THEN** `scroll_offset` 按上述规则变化(纯逻辑断言)

#### Scenario: 行级 / 鼠标滚轮步进与触底恢复跟随(逻辑可测)

- **WHEN** 调 `scroll_up`(行级)上滚若干行,再 `scroll_down` 步进直至触底
- **THEN** 上滚后 `follows_bottom` 为假且 offset 按行级步进变化;触底后 `follows_bottom` 恢复为真

#### Scenario: Ctrl+Home 到顶 / Ctrl+End 回底,裸 Home/End 不滚动(逻辑可测)

- **WHEN** 在跟随底部态调 `scroll_to_top`(`Ctrl+Home`),随后调 `scroll_to_bottom`(`Ctrl+End`)
- **THEN** `scroll_to_top` 后 `scroll_offset == 0` 且 `follows_bottom == false`;`scroll_to_bottom` 后 `follows_bottom == true`、`visible_scroll_offset` 回到底部偏移;裸 `Home`/`End` 不改 `scroll_offset`

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

`render` SHALL 渲染命令产出的 transcript 块(`设计规范/03`):C8 帮助块(两列 `cmd` + `desc`,**6 个帮助条目** —— `/login` / `/logout` 随 auth 迁至 CLI `mysteries auth` 移除;条目含 `/model` 查看与 `/model <name>` 切换两行、不含 `/compact`)、C9 快照块(`provider · model · iter X/maxIter · N msgs · cwd · tools: 7`,其中 `tools: 7` 指 7 个内置**工具**、与命令计数无关、**不变**)、notice 块(info / 占位提示,`info.fg` / 框)。带色,复用 `Theme` + `buffer_to_styled`。

#### Scenario: 帮助块与快照块带色快照

- **WHEN** transcript 含一个 C8 帮助块 / 一个 C9 快照块时渲染
- **THEN** 各自 `insta` 带色快照与锁定一致(C8 两列对齐 6 个帮助条目、**不含** `/login` `/logout`;C9 含 provider/model/iter/msgs/cwd/tools 字段)

### Requirement: 状态行常驻 meta

**底部状态行** SHALL 常驻显示 `provider · model · iter X/maxIter · N msgs · cwd`(`设计规范/02` C10)。phase label 已移至活动状态行,底部状态行 MUST NOT 再含 phase(原「与左侧 phase 并存」不再适用)。`iter` 由 UI 统计当前轮的 `StatusChanged(CallingModel)` 次数得到(新轮 / `TurnComplete` 重置),`msgs` = `transcript` 中**对话块(`User` / `Assistant`)**数(`Tool` 块与命令产出块 Help / Status / Notice **不计入**);其余取 session 快照(`/model` 切换后 model 同步更新)。

#### Scenario: 底部状态行 meta 快照(不含 phase)

- **WHEN** 给定 session 快照(provider/model/maxIter/cwd)与若干 transcript 块(含 `Tool` 块)渲染
- **THEN** 底部状态行带色快照含 `provider · model · iter X/maxIter · N msgs · cwd`,**不含** phase label,其中 `msgs` 只计 `User` / `Assistant` 块(不含 `Tool`),与锁定一致

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

`tui` 的 `UserInput` SHALL 增 `Interrupt` 变体、`AgentEvent` SHALL 增 `Interrupted` 变体。`run_agent_task` 的 `Prompt` 分支 MUST 以 `tokio::select!` 把本轮 `Agent::run_observed` 与一个**独立**中断信号并置:中断信号 MUST NOT 复用 `input_rx`。中断到达即 drop 本轮 run future,向 UI 发 `AgentEvent::Interrupted`、状态回 `Idle`,**agent task 与程序均存活**;中断后 MUST NOT 再次调用 provider。**中断路径 MUST 只发 `Interrupted`、不再紧跟冗余的 `StatusChanged(Idle)`**;且 **`apply(StatusChanged(Idle))` MUST NOT 置 `phase=Ready`**——`phase→Ready` 统一由终止 / 完成事件(`TurnComplete`/`Interrupted`/`Error`/`CompactDone`)驱动,消除 Idle 制造的闪帧、`new_message_count` 误增与直发窗口(`bump_new_message_count` 相应移到终止事件分支)。

UI 端 Esc / `Ctrl+C` 按**分流**(仅 `KeyEventKind::Press`,**模态优先于选区**):`pending_permission` 存在 → Esc 拒绝授权 / `Ctrl+C` 维持原行为(**最高优先**);否则存在选区 → Esc 清除选区 / `Ctrl+C` 复制并保留选区;否则**硬模态 `models_picker` / `session_picker` 或软浮层 `command_completion` 活跃 → 取消排队 / 退出分流 MUST NOT 接管**,Esc / `Ctrl+C` 归既有模态/浮层路由(picker 自消费**所有键**、补全浮层 Esc 关闭),不投 `Interrupt`、不退出、不记 `last_cancel_at`;否则**存在排队(`pending_queue` 非空)→ 两级取消(时间窗)**:以 `last_cancel_at` 计 `gap`——`gap >= CANCEL_DOUBLE_TAP`(默认 600ms)→ 投 `Interrupt` + 记 `last_cancel_at=now`,`gap < CANCEL_DOUBLE_TAP`(**快速连按**)→ `clear_queue()` + 投 `Interrupt`;否则**本轮运行中(无排队)→ 投 `Interrupt`**(**Esc 与 Ctrl+C 同**——现状 code `should_exit` 对运行中 Ctrl+C 未追平此契约、本 change 令 Ctrl+C 亦经 `phase.is_running()` 投 `Interrupt`);否则**就绪(无排队)**:**Esc → 退出程序**;**Ctrl+C → 空闲双击退出守卫**——首次记 `last_exit_intent_at` + 活动行提示「再按一次 Ctrl+C 退出」、**不退**,`EXIT_DOUBLE_TAP`(=1s)内再按 → 退出,超时未再按 → 提示消失、重置;判定 SHALL 抽纯函数 `exit_intent_action(gap, threshold) -> {Consumed, Exit}`(`gap < threshold` → `Exit`、`gap >= threshold`(含 `==`)→ `Consumed`),exit-intent 提示优先级 SHALL 高于 `copy_hint`(上膛警告不被遮)。取消判定 SHALL 抽纯函数 `cancel_action(gap, threshold) -> {InterruptAndAdvance, ClearAll}`(`gap>=threshold`→前者),`Instant` 只在事件循环算 `gap`;**推进 MUST NOT 触碰 `last_cancel_at`**。**排队由 app 层 `pending_queue` 持有,channel 恒最多一条**。优先级:pending > 选区 > 硬模态/软浮层(`models_picker` / `session_picker` / `command_completion`) > 有排队两级取消 > 运行中中断(无排队,Esc/Ctrl+C 同) > 就绪(Esc 退出 / Ctrl+C 双击退出)。(`Phase::Compacting` 视同运行态入本分流;压缩不可中断为 v1 Non-Goal。)

#### Scenario: 运行中中断以 Interrupted 收场且不再调用 provider

- **WHEN** 以 Mock provider(在 `complete` 中挂起的脚本)驱动 `run_agent_task`,投入 `Prompt` 后再投 `Interrupt`
- **THEN** 本轮以 `AgentEvent::Interrupted` 收场、状态回 `Idle`,provider 不被再次调用,agent task 继续存活;中断路径只发 `Interrupted`(Interrupted 后短窗内无任何尾随事件,须有断言锁定)

#### Scenario: 中断不消费排队的 Prompt

- **WHEN** 中断信号经独立通道到达,而 `input_rx` 中另有已 send 的 `Prompt`
- **THEN** 仅本轮被中断;`input_rx` 中的 `Prompt` 不被中断臂吞掉,后续正常消费

#### Scenario: 两级取消时间窗(快速连按清空 / 隔久单按判第 1 次)

- **WHEN** running 且 `pending_queue=["b","c"]`:① 第 1 次 Esc → 中断当前 + 记 `last_cancel_at`(推进 pop `b`);② `gap < 600ms` 内紧接再按 Esc;③ 另测:第 1 次 Esc 后隔 `gap >= 600ms` 才再按 Esc
- **THEN** ②(快速连按)→ `clear_queue()` 清空所有排队 + `Interrupt`;③(隔久单按)→ 判"第 1 次"(中断当前 + 推进下一条),**不**清空排队;推进不改 `last_cancel_at`

#### Scenario: cancel_action 纯函数判定(可单测)

- **WHEN** 对 `cancel_action(gap, threshold)` 分别给 `gap >= threshold`、`gap < threshold`
- **THEN** 分别返回 `InterruptAndAdvance`、`ClearAll`;判定不触碰 `Instant`,仅比较 `Duration`

#### Scenario: 运行中 Ctrl+C 中断(追平基线)

- **WHEN** agent 运行中(`phase.is_running()`、无排队)按 Ctrl+C
- **THEN** 投 `Interrupt` 中断当前轮(与 Esc 同),不退出程序

#### Scenario: 就绪 Ctrl+C 首次不退仅提示

- **WHEN** 就绪态(无排队/选区/模态)首次按 Ctrl+C
- **THEN** 不退出,活动行显示「再按一次 Ctrl+C 退出」,记 `last_exit_intent_at`

#### Scenario: 就绪 Ctrl+C 阈值内连按退出

- **WHEN** 就绪 Ctrl+C 后 `EXIT_DOUBLE_TAP` 内再按 Ctrl+C
- **THEN** 退出程序

#### Scenario: 就绪 Ctrl+C 超时重置

- **WHEN** 首次 Ctrl+C 后超过 `EXIT_DOUBLE_TAP` 未再按,再单按 Ctrl+C
- **THEN** 又只提示、不退出

#### Scenario: exit-intent 提示不被 copy 遮

- **WHEN** 复制(`copy_hint` 活跃)后紧接就绪 Ctrl+C 上膛
- **THEN** 活动行显示 exit-intent 提示(优先于 `copy_hint`),守卫可见

#### Scenario: exit_intent_action 纯函数判定(可单测)

- **WHEN** 对 `exit_intent_action(gap, threshold)` 分别给 `gap < threshold`、`gap >= threshold`
- **THEN** 分别返回 `Exit`、`Consumed`;边界 `gap == threshold` → `Consumed`

#### Scenario: Esc 分流(模态优先于选区,含取消排队)

- **WHEN** 分别在「pending + 有选区」/「有选区、无 pending」/「有排队、运行中、gap≥阈值」/「有排队、gap<阈值」/「本轮运行中、无排队」/「就绪、无排队」下收到 Esc(Press)
- **THEN** 依次:回送 `Deny` / 清选区 / 投 `Interrupt` + 记 last_cancel_at / `clear_queue()` + 投 `Interrupt` / 投 `Interrupt`(无排队中断)/ 退出程序;优先级 pending > 选区 > 硬模态/软浮层 > 有排队两级取消 > 运行中中断 > 就绪退出

#### Scenario: 有排队时浮层的 Esc 不被取消排队劫持

- **WHEN** running 且 `pending_queue` 非空,分别在 `models_picker` 打开、`command_completion` 浮层活跃时按 Esc(Press)
- **THEN** Esc 归 picker / 补全浮层,**不**投 `Interrupt`、**不**记 `last_cancel_at`、**不**清排队

#### Scenario: 中断态渲染为非致命 notice

- **WHEN** UI 收到 `AgentEvent::Interrupted` 后渲染
- **THEN** transcript 末尾含一条「⊘ 已中断本轮」notice 块(`info.fg` / 非致命),与锁定带色快照一致

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

`render` SHALL 提供按显示宽度的 transcript 文本排版:① `User` / `Assistant` 文本块按视口宽度**换行**,续行**悬挂缩进**对齐 marker 宽度;② `Assistant` marker 为 `◆ `、`User` marker 为 `> `;③ 宽度度量 `display_width` MUST 把 CJK / 全角与常见 emoji 记为 2 列、组合 / 零宽字符记为 0 列(`display_width` / `char_width` 置于中立 `tui/width.rs`,供 render 与文本缓冲 reducer 共用);④ 输入框 MUST 把终端光标定位到 **cursor 所在 `(逻辑行, display 列)`** 经 logical→visual 映射后的位置(**多行编辑下光标不再恒在输入串末尾**);⑤ C2 欢迎态(`设计规范/03`)文本**水平居中**,空会话时整体**垂直留白**居中。以上 MUST 可经 `TestBackend` 渲染断言 / `insta` 快照验证。

#### Scenario: 显示宽度度量

- **WHEN** 计算 `display_width("a")` / `display_width("你好")` / `display_width("👋")`
- **THEN** 分别为 `1` / `4` / `2`

#### Scenario: Assistant 块换行 + 悬挂缩进 + 宽度

- **WHEN** 一个超视口宽度、含 emoji 的 `Assistant` 块渲染到窄 `TestBackend`
- **THEN** 首行以 `◆ ` 起、续行按 marker 宽度悬挂缩进对齐,文本按显示宽度换行(不串列、不撑破视口)

#### Scenario: 输入框光标定位到 cursor 位置

- **WHEN** 单行输入 `你好`(cursor 在末尾)时渲染;另在多行输入、cursor 在**第 1 逻辑行行首**时渲染
- **THEN** 前者光标位于 `prompt + 你好显示宽度` 之后的列;后者光标位于第 1 显示行、prompt 之后的行首列(不再恒定位到输入串末尾)

#### Scenario: 欢迎态居中(快照)

- **WHEN** 空会话渲染到 `TestBackend`
- **THEN** 带色快照中 C2 欢迎态文本水平居中、整体垂直留白居中,与锁定快照一致

### Requirement: 按键事件去重(仅 Press)

TUI 按键处理 MUST 仅响应 `KeyEventKind::Press`:`on_key` / `should_exit` / 滚动键处理 SHALL 忽略 `Release` / `Repeat` 事件,避免 Windows 终端每次按键三发(Press/Repeat/Release)导致的重复输入与误触发。

#### Scenario: 非 Press 事件被忽略

- **WHEN** 对同一字符依次投入 `Press` / `Release` / `Repeat` 三个 `KeyEvent`
- **THEN** 仅 `Press` 生效(输入串只增一个该字符);`Esc` 的 `Release` / `Repeat` 不触发退出

### Requirement: 工具输出折叠与全局展开(ctrl+o)

`AppState` SHALL 持折叠态 `tools_expanded: bool`(默认 `false` = 折叠)。`render` MUST 据 `tools_expanded` 对每个 `TranscriptBlock::Tool(ToolCard)` 块二选一渲染:**折叠**态渲染 `设计规范/03` C5 的**单行头**(状态 glyph + 工具名 + args 摘要 + 结果摘要)+ **diff 体**(仅 write_file / edit_file 且 diff 非空,折叠配额 `DIFF_COLLAPSED_MAX_ROWS`,见「工具卡 C5 渲染」),**不**渲染 output 体与脚,且单行头 **MUST NOT 含** `┌─` 边框前缀(`┌─` / `└─` 边框仅展开态用);**展开**态渲染**全量**(头 / diff 体(仅 write_file / edit_file 且 diff 非空,展开配额,见「工具卡 C5 渲染」)/ 体 / 脚 + 截断标记)。折叠行结果摘要 SHALL 按序判定:`running` → ` · 运行中…`;携 `exit` → ` · exit {code}`(非 0 用 `error.fg`);`done` 且为 write_file / edit_file 且 diff 非空 → ` · +A −D ⌄`(定义与着色见「工具卡 C5 渲染」);其余 output 非空 → ` · {N} 行 ⌄`(N = output 行数,`⌄` 提示可展开)。

连续的 `Tool` 块 SHALL 视为一组:**组内相邻 `Tool` 块之间 MUST NOT 插入空行**(紧凑呈现);组边界(相邻块非 `Tool`,或位于 transcript 端点)仍插入空行分隔。折叠态下,每个连续 `Tool` 组的**组首**卡片 SHALL 在结果摘要后追加 ` · ctrl+o 展开`(`text.muted`);**组内非组首**卡片与**展开**态 MUST NOT 追加该提示(每组仅一次、补展开可发现性)。

`ctrl+o`(`KeyCode::Char('o')` + `KeyModifiers::CONTROL`,**仅** `KeyEventKind::Press`)MUST 翻转 `tools_expanded`(全局展开/折叠**所有**工具卡),且 MUST NOT 把 `o` 写入输入框(在文本输入 arm 之前拦截)。折叠**仅作用于** `Tool` 块;`User` / `Assistant` / `Error` / `Help` / `Status` / `Notice` 块 MUST NOT 受折叠影响。本期**只**提供全局 toggle,不提供单条卡片独立展开。

#### Scenario: 工具卡默认折叠为单行(带色快照)

- **WHEN** 一个 `done` 且 output 多行、**diff 为空**(非 write/edit 工具,或 write/edit 而缺齐 diff 参数)的 `Tool` 块(transcript 中唯一,即所在组组首)在 `tools_expanded == false`(默认)下渲染到 `TestBackend`
- **THEN** 带色快照仅含该卡的单行头(glyph + 工具名 + args + ` · {N} 行 ⌄` + ` · ctrl+o 展开`),**不**含 `┌─` 头边框、output 体行与 `└─` 脚,与锁定一致(diff 非空的 write/edit 折叠卡另见「工具卡 C5 渲染」的折叠态 diff 体 scenario)

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

键盘 SHALL 提供 transcript 的导航且**不依赖**鼠标捕获:整页(`PageUp` / `PageDown`)、到顶(`Ctrl+Home`)、回底并恢复跟随(`Ctrl+End`)合起来 MUST 能从任意位置到达 transcript 的**顶**与**底**。**`↑` / `↓` 不用于 transcript 滚动**——在主输入态改为多行光标 / 输入历史导航(见「输入历史 ↑↓ 召回」);**裸 `Home` / `End` 不用于 transcript 滚动**——改为输入行内光标(见「多行输入编辑」)。故纯键盘滚动以**页级 + 边界**(`PageUp` / `PageDown` / `Ctrl+Home` / `Ctrl+End`)覆盖到顶 / 底;**不再保证逐行键盘滚动**,逐行仅在转发滚轮的终端经鼠标滚轮提供(ConPTY 无滚轮时只能页级遍历——此为在 ↑↓ / Home-End 抢键冲突上的取舍)。鼠标滚轮(`MouseEventKind::ScrollUp` / `ScrollDown`)SHALL 作为**尽力而为**的增强:在转发滚轮事件的终端可用;在 **ConPTY 不转发滚轮的 Windows 构建**上滚轮事件不到达 crossterm,此失效 MUST NOT 削弱键盘的页级 + 边界覆盖(滚轮缺失时仍可纯键盘到达顶 / 底)。`scroll_up` / `scroll_down` 原语 MUST 保留供页级实现与滚轮复用。`terminal.rs` 的鼠标捕获 MUST 保持开启(失效根因在平台而非捕获缺失,不因本 change 关闭捕获)。

**捕获重申(子进程冲击恢复)**:子进程 attach console 可能重置输入模式,使终端把滚轮**降级为 ↑/↓ 方向键**(表现为滚轮失效、且方向键误入多行光标 / 输入历史路径)。事件循环 SHALL 在 ui_rx 处理完 `AgentEvent::ToolCallFinished` 后幂等重发 `EnableMouseCapture`(`TerminalGuard::reassert_mouse_capture()`);重申对已开启状态无副作用。根因侧的子进程 console 独立性见 builtin-tools「run_shell 执行」。

#### Scenario: 纯键盘到顶与回底(无任何 MouseEvent)

- **WHEN** transcript 行数超视口、跟随底部态,**不**投入任何 `Event::Mouse`,仅以键盘调 `scroll_to_top`(`Ctrl+Home`)再 `scroll_to_bottom`(`Ctrl+End`)
- **THEN** `scroll_to_top` 后 `visible_scroll_offset` 指向顶(0)、`follows_bottom` 为假;`scroll_to_bottom` 后 `follows_bottom` 为真且回到底部偏移

#### Scenario: ↑ / ↓ 与裸 Home / End 不再滚 transcript

- **WHEN** 主输入态(无浮层)按 `↑` 或裸 `Home`
- **THEN** transcript 滚动位置不变;`↑` 归多行光标/输入历史、裸 `Home` 归输入行首光标
- **WHEN** 需要键盘滚 transcript 到顶
- **THEN** 用 `Ctrl+Home`(到顶)或 `PageUp`(页级),`↑` / `↓` / 裸 `Home` / `End` 不参与滚动

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

### Requirement: 会话 history 跨轮累积

TUI agent task SHALL 维护跨 prompt 累积的会话 history(`System` + 历轮 `User`/`Assistant`/工具消息),**MUST NOT** 每投入一个 `UserInput::Prompt` 就从仅含 `System` 的空 history 重建。每轮 prompt 在既有 history 末尾追加当前 `User`,跑完 `Agent.run` 后将 working history 写回共享状态;下一轮 provider 请求 MUST 携带此前各轮消息。`/compact` 作用于该共享会话 history(与自动压缩同一 `Compacting` 逻辑)。

#### Scenario: 两轮后第二轮请求含第一轮消息

- **WHEN** 以 Mock provider(脚本:两轮各返回一段文本)驱动 agent task,连续投入两个 `UserInput::Prompt`
- **THEN** 第二轮发给 provider 的 messages 含第一轮的 `User` 与 `Assistant` 原文,共享会话 history 亦保留两轮完整记录;全程无终端

### Requirement: / 命令补全

输入串以 `/` 起头且仍在**命令名输入中**(尚无空格)时,系统 SHALL 渲染命令补全浮层:列出**前缀匹配**的内置命令(名 + 简述),高亮当前选中项。`↑` / `↓` SHALL 移动高亮;`Tab` 或 `Enter` SHALL 以选中命令名补全输入框;`Esc` SHALL 关闭浮层(不清空输入);继续输入字符 SHALL 重新过滤。补全候选 MUST 取自 builtin-commands 的命令元数据(与执行解析同一命令清单,避免漂移)。非 `/` 开头、或命令名已输完(含空格进入参数)时 MUST NOT 显示浮层。

#### Scenario: 输 / 弹前缀匹配候选

- **WHEN** 输入框内容为 `"/co"`
- **THEN** 补全浮层列出前缀匹配的命令(含 `/compact`)及其简述

#### Scenario: Tab 补全选中项

- **WHEN** 浮层高亮候选为 `/compact`,按 `Tab`
- **THEN** 输入框补全为 `"/compact"`,浮层关闭

#### Scenario: 非命令态不弹浮层

- **WHEN** 输入框为 `"/model gpt"`(已进入参数)或 `"hello"`(非 `/` 起头)
- **THEN** 不显示补全浮层

### Requirement: 活动状态行(输入框上方)

`render` SHALL 在**输入框上方**渲染单行**活动状态行**,承载动态工作状态,由左到右:`spinner`(running 态依 `AppState.spinner_frame` 取帧;`Ready`/`WaitingForPermission` 用静态 glyph)+ phase 文案(见「全 phase 状态行 C10」)+ 运行中(phase 非 `Ready` 且非 `WaitingForPermission`)的 **esc 中断提示** + **token 速率** `↓ N tok · X t/s`(见「token 用量累计与速率呈现」)。活动状态行 MUST **恒占 1 行**(不随 `Ready`/running 改变其行高,避免 transcript 视口高度跳动)。`Ready`/`Idle` 态 MUST **简显**(不显 spinner 动画与 esc 提示)。配色沿用 `设计规范/02` 状态机(running=`accent.primary`、`WaitingForPermission`=`warning.fg`、idle / 速率=`text.muted`)。phase label MUST 出现在活动状态行、MUST NOT 出现在底部状态行。渲染 MUST 可经 `TestBackend` / `insta` 带色快照验证。

#### Scenario: 运行中活动行含 spinner + phase + esc 提示

- **WHEN** `phase = CallingModel`、`spinner_frame` 取某固定帧时渲染到 `TestBackend`
- **THEN** 输入框上方的活动状态行带色快照含 `{spinner} 调用模型…` 与 esc 中断提示(含 `esc`),与锁定一致;底部状态行不含 phase

#### Scenario: Ready 态活动行简显

- **WHEN** `phase = Ready` 时渲染
- **THEN** 活动状态行简显(不含 spinner 动画与 esc 提示),与锁定带色快照一致

#### Scenario: 活动行恒占一行(布局稳定)

- **WHEN** 分别在 `phase = Ready` 与 `phase = CallingModel` 下渲染同尺寸 `TestBackend`
- **THEN** 两态 transcript 视口高度相同(活动状态行高度不变,布局不跳动)

### Requirement: token 用量累计与速率呈现

`tui` 的 `AgentEvent` SHALL 增 `Usage { input_tokens: u32, output_tokens: u32 }` 变体;`ChannelObserver` MUST impl `on_usage` 将每轮 `Usage` forward 为 `AgentEvent::Usage`。`AppState` SHALL 提供纯函数 `record_usage(usage: Usage, elapsed: Duration)`:累计本轮 token 用量(用于 `↓ N tok`,默认取 `output_tokens` 累加)并计算速率(`X t/s` = `output_tokens as f64 / elapsed.as_secs_f64()`;`elapsed` 为 0 时速率为 `None`、MUST NOT 除零 / panic)。`record_usage` MUST NOT 持 `Instant` 或读系统时钟 —— `elapsed` 由调用方传入(守既有 spinner「不把时间引入 `AppState` / `render`」契约),使其可注入合成 `Usage` + `Duration` 单测。`TurnComplete` 与新一轮 `Prompt` MUST 重置本轮累计(与 `iteration` 同语义)。活动状态行 SHALL 显示 `↓ N tok · X t/s`(无可用 usage 时 MUST NOT 显示臆造速率)。**实时流式 t/s 不在本能力范围**:provider `usage` 仅在每次 `complete` 完成后回传、`on_text` 无 token 计数,故速率在**每次 model 调用完成后**刷新,非流式途中逐 token 跳动。

#### Scenario: record_usage 累计 token 并算速率(纯函数)

- **WHEN** 依次 `record_usage(Usage{output_tokens:120,..}, 2s)` 与 `record_usage(Usage{output_tokens:60,..}, 1s)`
- **THEN** 本轮累计 `↓ N tok` 为 `180`,最近速率为 `60.0 t/s`(`60/1`);全程不读系统时钟(elapsed 为入参)

#### Scenario: TurnComplete 重置本轮累计

- **WHEN** `record_usage` 累计若干后 `apply(AgentEvent::TurnComplete)`
- **THEN** 本轮累计 `↓ N tok` 归 0(下一轮重新累计)

#### Scenario: elapsed 为 0 不算速率不 panic

- **WHEN** `record_usage(Usage{output_tokens:50,..}, Duration::ZERO)`
- **THEN** token 累计含 50,速率为 `None`(活动行不显 `t/s`),无除零 / panic

### Requirement: 流式字符估算近似 t/s(标 ~)

流式途中(`CallingModel` 且尚未收到本轮 `Usage`)SHALL 据 `DeltaSink::on_text` 累加的**字符数**与 UI 侧 elapsed(同 Q6,`Instant` 在 IO task)经**纯函数**粗估 token(如 `chars/4`,项目无 tokenizer、**MUST NOT** 伪装为精确值)并计算近似速率,活动行显示 **`~X t/s`**(前缀 `~` 表示估算)。`elapsed` 为 0 时近似速率 MUST 为 `None`、不 panic。收到本轮真实 `AgentEvent::Usage` 并经 `record_usage` 后 MUST **校正**:去掉 `~`,改显真实 `X t/s`(方案 A)。无字符 / 无 elapsed 时 MUST NOT 臆造速率。

#### Scenario: 流式估算标 ~ 且完成后校正

- **WHEN** 流式累计 400 字符、elapsed=2s(尚未收到 usage),随后 `record_usage(Usage{output_tokens:120,..}, 2s)`
- **THEN** 流式阶段活动行含 `~50 t/s`(400/4/2 量级粗估,标 `~`);`record_usage` 后改显真实 `60 t/s`(120/2)、无 `~`

#### Scenario: 流式估算 elapsed 为零不 panic

- **WHEN** 纯函数 `estimate_streaming_rate(100, Duration::ZERO)`
- **THEN** 返回 `None`,无除零 / panic

### Requirement: `UserInput::SetProvider` 变体与 agent-task 热替

`UserInput` SHALL 新增 `SetProvider { id: String, model: String }` 变体,对称于既有 `SetModel`。`run_agent_task` SHALL 持有「全部已配 provider profiles」(启动时由 `resolve_provider_profiles` 解析)与重建凭据的能力,并新增处理 arm:收到 `SetProvider{id, model}` 后,按 `id` 取 profile、组瞬时运行配置(继承启动配置的 timeout / 压缩旋钮)、重建 `CredentialChain` 经 `select_provider` 造新 `Arc<dyn Provider>`,热替进 `agent`(及手动 `compacting`)并同步 model。切换不打断既有会话 history。

#### Scenario: 收到 SetProvider 后下一轮用新 provider

- **WHEN** agent-task 收 `SetProvider{ id, model }`(id 在 profiles 中、凭据齐备),随后收 `Prompt`
- **THEN** 该轮模型请求经新 provider 发出、用新 model;会话 history 跨切换保留

#### Scenario: 未知 id 发 Notice 不崩

- **WHEN** 收 `SetProvider{ id }` 而 `id` 不在 profiles 中
- **THEN** 上送 `AgentEvent::Notice`(提示未知 provider),保持当前 provider,task 不退出

#### Scenario: 缺凭据切换发 Notice 不崩

- **WHEN** 目标 provider 缺 API key,`select_provider` 报错
- **THEN** 上送 `AgentEvent::Notice`(提示凭据缺失),保持当前 provider,task 不退出

### Requirement: 模型 picker 浮层

系统 SHALL 提供由 `/models` 触发的模态模型 picker 浮层。

**数据来源**:取自已配 provider profiles(`provider_profiles_from_paths`)× 内置目录(`models_for`):逐家 provider,`models_for(id)` 为 `Some` → 列**目录全部**模型;为 `None`(custom)→ 列其 profile **已配的那个 model**。SHALL 标记**当前 active** 的 `(provider, model)` 行。

**布局**:分组 —— provider 名为**不可选标题行**,模型缩进列其下。

**键位 / 交互**:`↑↓` 在**模型行**间移动(跳过标题行、首尾环绕);键入字符 / Backspace 实时**过滤**(不区分大小写 substring,匹配 `"{id}/{model}"`),过滤后高亮重置到**首个可见模型行**;`Enter` 选中高亮模型 → 发 `UserInput::SetProvider{ id, model }` 并关闭浮层;`Esc` 取消关闭(**不发消息**)。picker 打开时 SHALL **独占** `↑↓ / Enter / Esc / 字符 / Backspace`(优先于命令补全、transcript 滚动)。无匹配时 SHALL 显示空提示,`Enter` 为 no-op。

**测试边界**:构建(profiles×catalog→分组行)、过滤、`↑↓` 归约、`Enter`→选中 `(id, model)` 等 MUST 为**可单测纯逻辑**(不依赖真实终端);浮层渲染走 **insta 快照**。浮层样式 adapt `设计规范/` C6 框式(box-drawing 描边、钉状态行上方、footer `↑↓ 选 · Enter 切 · Esc 取消` + 过滤串回显);新增组件登记 `设计规范/03` C12。

#### Scenario: 构建分组列表并标记当前 active(纯函数)

- **WHEN** 以 profiles `{ wps: {model="zhipu/glm-5.2"}, openai: {model="gpt-5.5"} }`、当前 active = `(wps, "zhipu/glm-5.2")` 构建 picker 行
- **THEN** 得分组行:`wps` 标题 + 其目录 8 个模型(含 `zhipu/glm-5.2` 标 ● 当前)、`openai` 标题 + `gpt-5.5`;标题行不可选

#### Scenario: custom provider 列其已配 model(纯函数)

- **WHEN** profiles 含 `my-llm`(`models_for("my-llm") == None`,`model = "x-1"`)
- **THEN** `my-llm` 组仅列一行 `x-1`(custom 无目录,用已配 model)

#### Scenario: ↑↓ 在模型行间移动、跳标题、环绕(纯函数)

- **WHEN** 对 picker 行(含标题与模型混排)施加 `↑` / `↓`
- **THEN** 高亮只落在**模型行**(跳过标题);末模型再 `↓` 环绕到首模型,首模型再 `↑` 环绕到末模型

#### Scenario: 输入过滤缩小列表并重置高亮(纯函数)

- **WHEN** picker 打开后键入 `glm`
- **THEN** 仅匹配 `"{id}/{model}"` 含 `glm` 的模型行(及其 provider 标题)可见,高亮重置到首个可见模型行;键入无匹配串则显示空提示

#### Scenario: Enter 选中发 SetProvider(纯函数 / 注入)

- **WHEN** 高亮落在 `(wps, "zhipu/glm-5")` 时按 `Enter`
- **THEN** 产生 `UserInput::SetProvider{ id: "wps", model: "zhipu/glm-5" }` 且 picker 关闭;空匹配下 `Enter` 为 no-op

#### Scenario: Esc 取消不发消息(纯函数)

- **WHEN** picker 打开时按 `Esc`
- **THEN** picker 关闭,**不**产生 `SetProvider`(当前 provider/model 不变)

#### Scenario: picker 渲染快照(insta)

- **WHEN** 以一组 profiles + 某高亮 + 过滤串渲染 picker 浮层(`TestBackend`)
- **THEN** 快照含分组(标题 + 缩进模型)、当前 active 标记、高亮行、footer 键位提示;与基线快照一致

### Requirement: 输入历史 ↑↓ 召回(本会话内存)

系统 SHALL 在**主输入态**(无浮层:无 pending 权限框、无 `/` 命令补全、无 models picker)下,把 `↑` / `↓` 经**合并动作 `Up` / `Down`** 按**多行光标优先、边界翻历史**分流(由文本缓冲 reducer 按 cursor 逻辑行位置分派):光标**不在首逻辑行**时 `Up` SHALL 上移一行、**不在末逻辑行**时 `Down` SHALL 下移一行(按 `display` 列对齐);光标在**首逻辑行** `Up` / **末逻辑行** `Down` SHALL 召回上/下一条**已提交输入**(普通 prompt 与 `/命令` 均入历史)。进入历史前的草稿(可为多行)MUST 被保存,游标越过最新一条时 `Down` SHALL 恢复该草稿;召回一条历史后 cursor MUST 置于文本末尾;在历史某条上**键入字符**或**提交** MUST 重置游标回草稿态。连续两次提交相同文本 MUST 只入历史一条。历史仅存于本会话内存,关闭 TUI 即清空(不落盘)。历史/光标导航为纯函数 reducer,可单测。命令补全 / picker 打开时 `↑↓` 归各自浮层处理(优先级最高,在投 `Up`/`Down` 之前拦截),历史与光标移动均不参与。

#### Scenario: 单行时 Up 逐条回溯、Down 前进

- **WHEN** 依次提交 `a`、`b`,在**单行**空输入态按 `Up`
- **THEN** 输入框为 `b`(首行==末行,`Up` 直接翻历史);再按 `Up` → `a`;在 `a` 上按 `Down` → 回到 `b`

#### Scenario: 多行时 Up/Down 先移光标、边界才翻历史

- **WHEN** 输入框含两逻辑行文本,光标在**第 2 行**按 `Up`
- **THEN** 光标上移到第 1 行(不翻历史);此时再按 `Up`(已在首行)→ 才召回上一条历史(存当前多行草稿);在末行按 `Down` 越过最新 → 恢复该多行草稿

#### Scenario: Down 越过最新恢复草稿

- **WHEN** 输入框已键入未提交草稿 `dr`,光标在末行按 `Up` 进入历史,再按 `Down` 越过最新一条
- **THEN** 输入框恢复为 `dr`

#### Scenario: 键入字符脱离历史

- **WHEN** 处于历史某条,键入一个字符
- **THEN** 游标回草稿态,该字符插入到光标处,后续首行 `Up` 从最新条重新回溯

#### Scenario: 连续重复提交去重

- **WHEN** 连续两次提交相同文本 `x`
- **THEN** 历史中只保留一条 `x`

#### Scenario: 浮层打开时 ↑↓ 不归历史也不移光标

- **WHEN** `/` 命令补全浮层打开,按 `↑↓`
- **THEN** 由补全浮层处理高亮移动,输入历史与文本光标均不变

### Requirement: 权限模式切换键与底部模式行

系统 SHALL 支持 `Shift+Tab`(`KeyCode::BackTab`)在 `Normal → AcceptEdits → Yolo → Normal` 间循环切换当前权限模式;切换 MUST 即时生效于后续工具决策(经共享模式句柄,与 agent-task 同一来源)。当前模式 SHALL 显示在**状态行下方一条独立的底部模式行**(屏幕最末行,**不**再占用状态行 C10),格式 `<glyph> <mode> · shift+tab 切换`,每模式带专属 glyph(`▸` Normal / `▸▸` AcceptEdits / `▲` Yolo)与 theme 配色(Normal `text.muted` / AcceptEdits `accent.primary` / Yolo `warning.fg`)。模式行 SHALL 常驻显示(含 `Normal`),令 `shift+tab` 提示可发现。状态行 C10 MUST NOT 再含模式段。切换键 SHALL 在任意 phase 可用(含 pending 权限框展示时,语义同 `ctrl+o`)。模式默认 `Normal`,不跨重启持久化。自动放行命中时不产生 pending 权限框(C6 不渲染),工具直接执行。

#### Scenario: Shift+Tab 循环切换

- **WHEN** 当前 `Normal`,按 `Shift+Tab`
- **THEN** 切到 `AcceptEdits`;再按 → `Yolo`;再按 → `Normal`

#### Scenario: 底部模式行反映当前模式

- **WHEN** 切到 `Yolo`
- **THEN** 屏幕最末行渲染 `▲ yolo · shift+tab 切换`(`warning.fg` 色),且状态行 C10 不含任何模式段

#### Scenario: Yolo 下改动类工具不弹权限框

- **WHEN** 当前 `Yolo`,模型调用一个 `Execute`(shell)工具
- **THEN** 不产生 pending 权限框(C6 不渲染),工具直接执行

### Requirement: 跳到底部提示与新消息计数

当 transcript **未跟随底部**(用户经 `PageUp` / `Ctrl+Home` 等滚离底部)时,系统 SHALL 在 transcript 视口底部、输入框上方钉一条单行提示 pill;**跟随底部时 pill MUST 隐藏**。pill 文案两态:自滚离底部以来无新增助手消息时为 `跳到底部 (ctrl+End) ↓`;新增了 N(≥1)条**已完成的助手消息**时为 `N 条新消息 (ctrl+End) ↓`。新消息计数 SHALL 只计助手消息(一轮答复 = 1),MUST NOT 计 user 回显 / 工具卡 / notice;计数在**未跟随底部**期间累加、回到底部跟随时 MUST 清零。`Ctrl+End` SHALL 使 transcript 回底并恢复跟随(**裸 `End` 归输入行内光标、不再回底**),回底后 pill 隐藏、计数清零。pill 渲染 SHALL 局部覆盖(仅 pill 宽,不留全宽黑带),配色用 theme token(adapt 设计规范 C14),含 `↓` glyph。计数增量逻辑 SHALL 为纯函数、可单测。

#### Scenario: 跟随底部时无 pill

- **WHEN** transcript 处于跟随底部态
- **THEN** 不渲染跳到底部 pill,新消息计数为 0

#### Scenario: 滚离底部显示「跳到底部」

- **WHEN** 用户经 `PageUp` 滚离底部,其间无新增助手消息
- **THEN** 视口底部渲染 `跳到底部 (ctrl+End) ↓`

#### Scenario: 滚离底部期间新助手消息累加(仅助手)

- **WHEN** 已滚离底部,模型完成 1 条助手回复(其间含若干工具卡)
- **THEN** pill 显示 `1 条新消息 (ctrl+End) ↓`(工具卡 / user 回显不计)
- **WHEN** 再完成 1 条助手回复
- **THEN** pill 显示 `2 条新消息 (ctrl+End) ↓`

#### Scenario: Ctrl+End 回底清零

- **WHEN** pill 显示 `2 条新消息 (ctrl+End) ↓`,按 `Ctrl+End`
- **THEN** transcript 回底并恢复跟随,pill 隐藏,计数清零

### Requirement: 鼠标滚轮滚动(捕获鼠标)

TUI SHALL 启用鼠标捕获:`TerminalGuard` 进入 alternate screen 时发 `EnableMouseCapture`,退出 / panic 时经 `restore_terminal` 发 `DisableMouseCapture`,使鼠标滚轮以 `Event::Mouse` 到达程序而非被终端翻译为 `↑/↓` 方向键。`MouseEventKind::ScrollUp` / `ScrollDown` SHALL 经 `scroll_up` / `scroll_down` 原语驱动 transcript 上 / 下滚动(每事件固定行数),**滚动时若存在选区 MUST 清除选区**(见「鼠标拖选与复制」)。`Down(Left)` / `Drag(Left)` / `Up(Left)` 用于 app 自管拖选复制(见「鼠标拖选与复制」);**除滚轮与上述选区用 kind 外的 mouse kind MUST 被忽略(不改交互)**。键盘 `↑↓` MUST NOT 受影响(仍归多行光标 / 输入历史)。鼠标捕获 MUST 在退出 TUI / panic 时正确解除(沿用 `restore_terminal` 单一路径,不残留鼠标模式)。**终端原生框选让位于 app 自管拖选复制(无需 Shift)**。

**降级**:部分 Windows ConPTY 构建即便捕获也可能不转发滚轮事件——此时滚轮无效,但 MUST NOT 影响键盘滚动(`PageUp` / `PageDown` / `Ctrl+Home` / `Ctrl+End`)与 `↑↓` 历史/光标(键盘全覆盖不受损)。

#### Scenario: 进入 TUI 启用鼠标捕获、退出解除

- **WHEN** 构造 `TerminalGuard` 进入 alternate screen
- **THEN** 终端被置入鼠标捕获模式(setup 发 `EnableMouseCapture`)
- **WHEN** TUI 退出或 panic,经 `restore_terminal`
- **THEN** `DisableMouseCapture` 被发出,终端恢复原生鼠标行为(无残留)

#### Scenario: 滚轮事件驱动 transcript 滚动并清除选区

- **WHEN** 收到 `Event::Mouse` 且 kind 为 `ScrollUp`
- **THEN** 经 `scroll_up` 原语上滚 transcript(固定行数),若有选区则清除
- **WHEN** 收到 `Event::Mouse` 且 kind 为 `ScrollDown`
- **THEN** 经 `scroll_down` 原语下滚 transcript,若有选区则清除

#### Scenario: 键盘 ↑↓ 不受滚轮捕获影响

- **WHEN** 鼠标捕获已启用,主输入态按键盘 `↑`
- **THEN** 仍归多行光标 / 输入历史(滚轮捕获不改键盘 `↑↓` 语义)

### Requirement: 鼠标拖选与复制(捕获鼠标下 app 自管选区)

在已捕获鼠标的全屏 alt-screen 下,TUI SHALL 由程序自管一段**线性选区**,使拖选复制与滚轮滚动、`↑↓` 共存且**无需按 Shift**。选区状态 MUST 为纯逻辑(与 ratatui 解耦)、可单测:`SelectionState { selection: Option<Selection>, dragging }`,`Selection { anchor: Point, head: Point }`,`Point { col, row }`(屏幕 cell 坐标)。`reduce_selection(state, action)` 归约:`Press(p)` 置锚点并起拖(`anchor=head=p, dragging=true`,若已有旧选区则被新拖选覆盖);`Drag(p)` 在拖拽中扩选(`head=p`);`Release(p)` 结束拖拽,**无位移(`anchor==head`,即单击)MUST 清除选区**、有位移 MUST 保留定稿选区;`Clear` 清空。归约 MUST NOT 产生副作用(复制由调用方按「归约后是否仍有选区」触发)。

事件循环 MUST 把 `MouseEventKind::Down(Left)/Drag(Left)/Up(Left)` 连同 `column/row` 映射为 `Press/Drag/Release`(现丢弃坐标的写法 MUST 改为透传坐标);`Up(Left)` 归约后仍有选区时 MUST 触发复制。**松开即复制且选区高亮保留**(不因复制而清除,与终端原生选区一致);**`Ctrl+C` 在有选区时亦复制并保留选区**(优先级见 MODIFIED「运行中可中断」:`pending_permission` 存在时不复制、维持原行为)。**复制本身 MUST NOT 清除选区**;清除选区的触发为:**新拖选(`Press`)/ 任意滚动(滚轮 `ScrollUp/Down` 与键盘 `PageUp/PageDown/Ctrl+Home/Ctrl+End`)/ 窗口 `Resize` / 提交输入(Enter)/ `/clear` / 单击未拖动 / `Esc`(有选区时)**。**裸 `Home`/`End` 为输入行内光标移动(见「多行输入编辑」),MUST NOT 清除选区**(它不滚动 transcript)。

选区文本 MUST 从**刚渲染的 buffer**(`terminal.draw` 返回的 `CompletedFrame.buffer`,非 `current_buffer_mut()`)读取,不得由 transcript + 布局反推。纯函数 `selection_text(buffer, sel) -> String` MUST:按 `col_range_for_row` 求每行列区间(单行 `start.col..=end.col`;跨行首行至行尾、中间行整行、末行至 `end.col`)、逐 cell 读 `symbol()`、**按 symbol 的显示宽度推进游标**——读到宽字符(display width = 2)后 positionally 跳过其后 `width-1` 个延续格(**延续格 symbol 实为单空格 `" "` 而非空串,MUST NOT 依赖 `is_empty()` 判定**;此即 ratatui 自身 diff / Buffer Debug 的做法)、每行 `trim_end`、行间 `\n` join。全屏下 `MouseEvent.column/row` 即 buffer cell 坐标(左上 `0,0`),直接映射。

选区高亮 MUST 由 `render` 末尾一趟 overlay pass 实现:遍历选区覆盖的 cell,把对应 cell 的 `bg` 置为新 token `selection.bg`(仅改背景、保留前景字形)。`Theme` MUST 加 `selection_bg` 字段,`midnight()` / `daylight()` 双调色板各给值,`tokens()` 计入,`theme.rs` 单测锁定二值。

**防 panic**:取文与高亮读写 cell MUST 用 `Buffer::cell` / `cell_mut`(返回 `Option`)或先把 `Point` clamp 到 `buffer.area`,MUST NOT 用裸 `Index`(越界会 panic);`MouseEvent.column/row` 由终端上报、不保证在界内,MUST 视为不可信坐标。复制经 `trait Clipboard { fn set_text(&mut self, text) -> Result<(), String> }` 注入,真实现包 `arboard`;复制失败(初始化 / 写入 `Err`)MUST **静默降级**为 `AgentEvent::Notice`(**不清除选区**),MUST NOT panic 或阻塞主循环。选区文本 `trim` 后为空(纯空白 / 空选区)或 `last_frame` 尚为 `None` 时,MUST **跳过复制**、不触碰系统剪贴板。v1 选区 MUST 限**当前可见视口**内(不做拖动自动滚 / 不选已滚出内容)。浮层(models_picker / 命令补全 / 权限框)打开时拖选 **按所见 cell 取文(WYSIWYG)**,不为浮层设特例(v1 不禁用拖选)。

#### Scenario: 选区归约起选 / 扩选 / 单击清除(纯逻辑)

- **WHEN** 对 `SelectionState::default()` 依次归约 `Press(2,1)` → `Drag(6,1)` → `Release(6,1)`
- **THEN** `Press` 后 `selection=Some{anchor:(2,1),head:(2,1)}, dragging=true`;`Drag` 后 `head=(6,1)`;`Release` 后 `dragging=false` 且选区**保留**;而 `Press(3,0)` → `Release(3,0)`(无位移单击)后 `selection=None`

#### Scenario: 选区规范化与逐行列区间(纯逻辑)

- **WHEN** `Selection{anchor:(6,2), head:(2,0)}` 求规范化及各行列区间(视口宽 W)
- **THEN** 规范化为 `start=(2,0), end=(6,2)`(reading order,start≤end);首行(row 0)列区间为 `2..W`、中间行(row 1)为 `0..W`、末行(row 2)为 `0..=6`;同行选区 `start.row==end.row` 时区间为 `start.col..=end.col`

#### Scenario: 从 buffer 取选区文本按显示宽度跳延续格(纯函数)

- **WHEN** 用 `Buffer::set_string` 真实写入含 CJK 宽字符的行(如「你好」占 4 列 + 尾随补白,延续格 symbol 为 `" "`),对覆盖该行的选区调 `selection_text`
- **THEN** 返回「你好」原文(读到宽字符后跳过其后 1 个延续格,**两个 CJK 间无多余空格**)、行尾补白被 `trim_end`、跨行以 `\n` 连接

#### Scenario: 选区高亮叠加背景色且松开后保留(insta 快照)

- **WHEN** 给定一段定稿(已 `Release`)选区,经 `TestBackend` 渲染 transcript 并叠加高亮
- **THEN** 带色快照中选区覆盖的 cell 背景为 `selection.bg`、前景字形不变,松开后高亮仍在,与锁定快照一致

#### Scenario: selection.bg token 双调色板锁值(theme.rs 单测)

- **WHEN** 读取 `Theme::midnight()` 与 `Theme::daylight()` 的 `selection_bg`
- **THEN** 二者为设计规范约定的固定值,`tokens()` 含 `("selection.bg", _)`,单测锁定

#### Scenario: 松开复制与 Ctrl+C 复制写入剪贴板且保留选区(注入 Clipboard)

- **WHEN** 以 mock `Clipboard` 注入(无 pending),拖出一段选区后 `Up(Left)`;另在有选区时按 `Ctrl+C`
- **THEN** 两种路径都调用 `Clipboard::set_text` 传入 `selection_text` 结果、**复制后选区与高亮保留**(不清除)、`Ctrl+C` 有选区时**不触发退出**;`pending_permission` 存在时 `Ctrl+C` 不复制(维持原行为)

#### Scenario: 复制失败静默降级为 Notice(注入失败 Clipboard)

- **WHEN** mock `Clipboard::set_text` 返回 `Err`,触发一次复制
- **THEN** transcript 末尾出现一条复制失败 `Notice`、不 panic、主循环继续、**选区保留**(不因复制失败而清)

#### Scenario: 空 / 纯空白选区跳过复制不触剪贴板

- **WHEN** 选区仅覆盖空白 cell(`selection_text` 经 `trim` 后为空),或 `last_frame` 尚为 `None` 时触发复制
- **THEN** 不调用 `Clipboard::set_text`、不触碰系统剪贴板、不 panic

#### Scenario: 滚动 / resize 清除选区,裸 Home/End 不清

- **WHEN** 存在定稿选区时收到 滚轮 `ScrollUp/Down`、或键盘 `PageUp/PageDown/Ctrl+Home/Ctrl+End`、或 `Event::Resize`;另在有选区时按裸 `Home`/`End`
- **THEN** 前者任一清除选区(`selection=None`,避免高亮 / 取文指向滚动或缩放后的错误内容);裸 `Home`/`End`(输入行内光标)**不清除选区**

#### Scenario: 越界坐标不 panic(纯函数 / 防御)

- **WHEN** 选区的 `Point` 超出 buffer 边界(终端上报越界坐标 / resize 失配),对其调 `selection_text` 或高亮 pass
- **THEN** 经 `Buffer::cell`/`cell_mut` 的 `Option` 短路或 clamp 安全处理,不 panic

### Requirement: 多行输入编辑(文本缓冲 + 光标 + 换行)

输入框 SHALL 支持**手动多行编辑**。核心为**纯逻辑文本缓冲**(与 ratatui 解耦、可单测):`text: String`(可含 `\n`)+ `cursor: usize`(字节位,**MUST 落在 char 边界**)+ 输入历史(list / index / draft)。归约 `reduce_*(state, action) -> state`,动作含 `InsertChar` / `InsertNewline` / `Backspace` / `Delete` / `MoveLeft` / `MoveRight` / `MoveLineStart` / `MoveLineEnd` / `Up` / `Down` / `SetText` / `PushSubmitted` / `InsertStr`(**`InsertStr` = 在光标处一次性插入整串正文,由 change `guard-paste-burst-submit` 加入,供批量 drain 合并粘贴正文、避免逐字符 O(n²) clone;插入后 cursor 到插入末尾、`history_cursor` 置 None,与 `InsertChar` 语义一致**)。行按 `\n` 切分;光标列以 `display_width` 计(宽字符感知)。`display_width` / `char_width` MUST 抽到中立模块(`tui/width.rs`)供缓冲 reducer 与 render 共用(纯逻辑不得反依赖 render 外壳)。

**换行 vs 提交**:`Ctrl+Enter`(`KeyCode::Enter` + `CONTROL`)/ `Shift+Enter`(`Enter` + `SHIFT`)/ `Ctrl+J`(`KeyCode::Char('j')` + `CONTROL`)SHALL 在光标处插入 `\n`;`Enter`(无 CONTROL/SHIFT)在**孤立敲击**(其所在事件批次里唯一的文本内容键)时 SHALL **提交**整段 `text`(trim 后为空则 no-op),在**粘贴突发批**(批内 ≥2 文本内容键)时 SHALL 作 `InsertNewline` 插入 `\n`、不提交(见「粘贴突发合并输入」requirement)。换行键判定 MUST 在通用 `KeyCode::Char(ch)` 插入分支**之前**;通用 `Char` 分支 MUST 过滤**带 CONTROL 且不带 ALT** 的字符(纯 `Ctrl+字符`,如 `Ctrl+J`,避免插入字面 `j`);**AltGr 合成字符(同时带 CONTROL+ALT)MUST 保留插入**(国际键盘)。设计 MUST NOT 依赖 kitty keyboard protocol(`PushKeyboardEnhancementFlags` 在 Windows 返 `Err`,会致 TUI 无法启动)、MUST NOT 改 `terminal.rs`。**命令解析仅单行**:提交时 `text` **含 `\n`** → 整段作 prompt(不解析命令);**不含 `\n`** 且以 `/` 起头 → 走 `parse_command`。

**光标导航**:`←` / `→` 按 char 边界步进(跨宽字符整步,不落字符中间);`Home` / `End`(无 CONTROL)到**当前行**首 / 尾;`Backspace` 删光标前一 char、`Delete` 删后一 char。`↑` / `↓` 由 app 投**合并动作** `Up` / `Down`,reducer 按 cursor **逻辑行**位置分派:cursor **不在首逻辑行**时 `Up` SHALL 上移一逻辑行、**不在末逻辑行**时 `Down` SHALL 下移一逻辑行(按 `display` 列对齐,落宽字符跨列中间取「**≤ 目标视觉列的最大 char 边界**」;不维护 goal-column);cursor 在**首逻辑行** `Up` / **末逻辑行** `Down` SHALL 翻输入历史(见「输入历史 ↑↓ 召回」)。**`Up`/`Down` 按逻辑行分派**:单条超宽逻辑行虽软换行成多显示行,其内部不逐显示行上下移(在该逻辑行即视为首/末行)——v1 限制。

**模态 vs 软浮层路由**:所有编辑/光标/换行键 MUST 落在既有守卫**之后**。**硬模态**(`pending_permission` / `models_picker`)活跃时,编辑/换行/光标键 MUST NOT 进入文本缓冲(归模态处理)。**软浮层 `command_completion`** 活跃时:`Char` / `Backspace` **仍 MUST 改文本缓冲并触发补全重过滤**(保持既有 `/` 补全「继续输入重新过滤」);`Up` / `Down` / `Tab` / `Enter` / `Esc` 归补全浮层;其余光标/换行键(`←→ Home End Delete` / 换行)在补全打开时 MUST NOT 破坏补全过滤(建议忽略或先关浮层再执行)。Tab 不插入 `\t`(仍归命令补全;无补全态 no-op)。

**渲染**:`render_input` SHALL 多行渲染 `text`;输入框内容高度 SHALL 随行数**动态、封顶**:`cap = clamp(屏高 - (顶栏3 + status_top_gap + permission_height + 活动1 + 状态1 + mode1 + input 边框2) - transcript_floor, 1, 绝对上限)`,`transcript_floor` 与 `rows[1] = Min(8)` 口径一致(=8);**cap MUST ≥ 1 内容行**(屏高极小时 input 保 1 行、必要时挤占 floor 而非算得 0)。超宽逻辑行 SHALL **软换行**;render MUST 做 **logical(逻辑行,列) → visual(显示行,列)** 映射,把终端光标 `set_cursor_position` 到 visual 坐标;**cursor 落逻辑行软换行边界(=某显示行满宽处)时 MUST 归入下一显示行行首**(唯一确定,供单测)。显示行 > cap 时框内滚动使**光标 visual 行**可见。缓冲归约、光标、宽字符列映射、命令仅单行判定、框高 cap、logical↔visual 映射 MUST 纯逻辑单测;多行渲染 + 光标 + 动态框高 + 软换行走 `insta` 快照。

#### Scenario: 换行键插入 `\n`、Enter 提交、命令仅单行(纯逻辑 + 接线)

- **WHEN** 光标在文本中,分别收到 `Enter+CONTROL`、`Enter+SHIFT`、`Char('j')+CONTROL`;另在含 `\n` 的多行 `text`(首行以 `/` 起)按**孤立**(其批次唯一文本键)的无 modifier `Enter`
- **THEN** 前三者在光标处插入 `\n`、不提交;含 `\n` 的整段作为 **prompt 提交**(不被 `parse_command` 当命令);单行且以 `/` 起头才走命令解析;`Ctrl+J` 不插入字面 `j`

#### Scenario: 光标导航与编辑(纯逻辑)

- **WHEN** 对含多行的 `text` 归约 `MoveRight`/`MoveLeft`/`Up`/`Down`/`MoveLineStart`/`MoveLineEnd`/`Backspace`/`Delete`
- **THEN** cursor 始终落 char 边界;`←→` 单 char 步进、`Home/End` 到当前行首/尾、`Up/Down` 到相邻逻辑行(多行内)、`Backspace/Delete` 删光标前/后一 char

#### Scenario: 宽字符光标与列对齐(纯逻辑,可判定 tie)

- **WHEN** 文本含 CJK 宽字符(如「你好」),做 `←→` 与跨行 `Up`/`Down`
- **THEN** `←→` 跨宽字符整步;跨行按 `display_width` 视觉列对齐,落宽字符跨列中间时取「≤ 目标列的最大 char 边界」,结果确定可测

#### Scenario: 单行 vs 多行时 Up/Down 分派(纯逻辑)

- **WHEN** 单行 `text` 光标按 `Up`;另在两逻辑行 `text` 光标在第 2 行按 `Up`、在第 1 行再按 `Up`
- **THEN** 单行(首行==末行)`Up` 直接翻历史;两行时第 2 行 `Up` 先上移到第 1 行(不翻历史),第 1 行再 `Up` 才翻历史(存多行草稿)

#### Scenario: 框高封顶、保底与极小终端(纯逻辑)

- **WHEN** 给定屏高与远超封顶的文本行数;另给定极小屏高
- **THEN** input 内容高 = `cap`(不超绝对上限、正常屏保 transcript ≥ 保底);极小屏时 `cap` 仍 **≥ 1**(不为 0),超出显示行经框内滚动到光标 visual 行可见

#### Scenario: 多行渲染 + 光标 + 软换行定位(insta 快照)

- **WHEN** 构造含普通多行与一条**超宽逻辑行**的 `text` 与某 cursor(含落在软换行满宽边界的 case),`TestBackend` 渲染
- **THEN** 快照显多行 + 超宽行软换行、框高随行数增大、光标在 logical→visual 映射后的正确 `(显示行, 列)`(边界处归下一显示行首);transcript 相应缩但不低于保底

#### Scenario: 硬模态吞键、软浮层补全仍过滤

- **WHEN** `pending_permission` 或 `models_picker` 活跃时收到换行/编辑/光标键;另在 `command_completion` 活跃时键入一个 `Char`
- **THEN** 硬模态下这些键归模态处理、MUST NOT 改文本缓冲;`command_completion` 下 `Char` **仍改缓冲并重过滤候选**(不被当作「浮层活跃即禁编辑」)

#### Scenario: 粘贴正文合并为 InsertStr(纯逻辑)

- **WHEN** 对文本缓冲归约 `InsertStr(s)`(`s` 可含多字节 CJK)
- **THEN** `s` 在光标处一次性插入、cursor 到插入末尾且落 char 边界、`history_cursor` 置 None;与逐个 `InsertChar` 得到相同 `text`,但只 clone 一次

### Requirement: 粘贴突发合并输入(批量 drain 防误提交)

TUI 事件循环 SHALL 在每次有 crossterm 事件到达时,用**同步** `event::poll(Duration::ZERO)` + `event::read()` 把当前**已就绪**的事件抽干成一个有界 batch(不 await、不阻塞、与 `EventStream` 共用同一 internal reader 故不污染其 waker),整批处理完只渲染一次;并按"一批**文本内容键**(`Char` + 裸 `Enter`,Press-only)的规模"区分**粘贴突发**与**用户敲击**:突发(批内 ≥2 文本内容键)内的裸 `Enter` SHALL 作换行插入,仅当裸 `Enter` 是其批次里唯一文本内容键时才判提交。

**提交前续读(防跨批/跨周期粘贴误提交)**:因 ConPTY 会把一次大粘贴切成多个 batch 分次投递(`drain` 一次 `poll(ZERO)` 只抽干当前已就绪即停),某换行可能落在此刻 `n==1` 的独立批 → 被误判提交。故 `drain` SHALL 在抽干 `poll(ZERO)` 后,当当前 batch 经 `classify` 将得 `Submit`(落单裸 `Enter`,谓词 `would_submit_lone_enter(batch)` 为真)时,以 `poll(GRACE)`(默认 `PASTE_CONTINUATION_GRACE = 10ms`,具名常量)做一次续读:窗口内有事件 → 读入**同一 batch**,**若读入的是非键盘事件(鼠标 `Moved`/焦点/resize)SHALL 收批**(不无限等待,避免 `EnableMouseCapture` 的高频 `Moved` 令续读不退出、阻塞重绘与 agent 流式),否则回到抽干循环(粘贴续批全是 `Event::Key`,带来 `Char` 后 `n` 变大、谓词转 false、循环终止);窗口内静默 → 收批(真提交)。以 `EVENT_BATCH_CAP` 在**抽干与续读两条路径**封顶防无限续读。续读触发 SHALL 抽为纯函数 `would_submit_lone_enter(batch) = classify_key_batch(press_key_events(batch))` 含 `Submit`;续批读入后 SHALL **复用既有** `classify_key_batch` / `apply_batch_input_key`(裸 `Enter` 因 `n` 变大自然从 `Submit` 变 `Newline`),**不新增 intent 改写逻辑**。此信号取自 `drain` 内同步 `poll`、**不经 `draw` / `select!` / 墙钟**,故不被渲染时延、agent 流式事件、鼠标事件污染。

硬模态在批处理中**逐键按当时活跃态**分治(非整批截断):`pending_permission` 活跃时首键应答后丢弃该批余下键;`models_picker` 活跃时每键透传给 picker(打字过滤/导航/选中 MUST NOT 丢失)。「剪贴板校准粘贴快路径」requirement MAY 在抽干后、合批/续读**前**拦截大粘贴批(命中即折叠 + 尾流校准丢弃;未命中时仅额外一次剪贴板惰性读取与一帧接收提示——该帧为本 requirement「整批处理完只渲染一次」的显式例外——其余行为不变)。此机制 SHALL NOT 依赖 bracketed paste(Windows crossterm 不产 `Event::Paste`,已诊断探针证实)、SHALL NOT 改 `terminal.rs`、SHALL NOT 改 `select!` 事件循环结构,复用既有文本缓冲的 `InsertNewline`/`InsertStr` 动作与既有 `on_key` 路由。**已知上限(Non-Goal)**:①**大 transcript 慢渲染下正常打字凑批**(末字符 + 提交 `Enter` 落同批 → `n≥2` → `Enter` 误判换行、再按一次即提交;本 requirement 不碰 `classify` 的 `n≥2`→`Newline` 逻辑,该限制原样保留);②粘贴以换行结尾(续读窗口内无续批 → 末 `Enter` 仍提交、不自动换行;快路径命中时尾 `Enter` 属尾流被校准丢弃、不提交,见「剪贴板校准粘贴快路径」);③续批间隔慢到 > `GRACE` 的极端(跨秒/极慢分段粘贴)使落单换行仍被判提交;④粘贴含 Tab 丢失(快路径命中时 Tab 随剪贴板原文保真,见「剪贴板校准粘贴快路径」;慢路径维持此限);⑤模态关闭后同批粘贴尾 `Enter` 被丢弃——均需更强的到达建模或 bracketed paste(本栈不可用)。

#### Scenario: 粘贴多行整段进入缓冲、不逐行提交

- **WHEN** 一段多行文本被粘贴,产生一批(≥2 个文本内容键)瞬时到达的 `Char`/`Enter` 事件
- **THEN** 该批内所有裸 `Enter` 作为 `InsertNewline` 插入缓冲,连续 `Char` 正文合并为 `InsertStr` 插入缓冲
- **AND** 全程不触发提交,transcript 不新增 user 块,不向 agent 发出 prompt
- **AND** 整批处理完只渲染一次(不逐事件渲染)

#### Scenario: 跨批粘贴续批的落单 Enter 经续读并入同批、判换行不提交

- **WHEN** 一次粘贴被 ConPTY 切成多批分次投递,某个换行此刻落在一个 `n==1` 的独立批里(`would_submit_lone_enter` 为真);其粘贴续批(下一段 `Char`/`Enter`)在 `poll(GRACE)` 续读窗口内到达
- **THEN** `drain` 的续读把续批读入**同一 batch** 并回到抽干循环,该 `Enter` 因 `n` 变大经**既有** `classify_key_batch` 判为 `Newline`、作 `InsertNewline` 不提交(而非因 `n==1` 走 `Submit` 自动发送)

#### Scenario: 续读触发判定为纯函数 would_submit_lone_enter(可单测)

- **WHEN** 对 `would_submit_lone_enter(batch)` 分别给 `[Enter Press]`、`[Enter Press, Enter Release]`、`[Char, Enter]`(n=2)、`[Char]`、空批,以及"`[Enter]` 续读粘入一个 `Char` 后"的批
- **THEN** 落单裸 `Enter`(前两者)→ `true`;`[Char,Enter]`/`[Char]`/空批 → `false`;粘入 `Char` 后 → `false`(经既有 `classify`,`Enter` 因 `n` 变大不再判 `Submit`);该谓词仅由 `press_key_events` + `classify_key_batch` 组合、不碰 `Instant`/IO

#### Scenario: 续读窗口读到非键盘事件即收批(防鼠标 Moved 拖住续读)

- **WHEN** 落单裸 `Enter`(`would_submit_lone_enter` 为真)进入续读,`EnableMouseCapture` 下 `poll(GRACE)` 窗口内读到 `Event::Mouse(Moved)`(或 `Focus`/`Resize`)
- **THEN** 该事件读入同批后 SHALL **立即收批、停止续读**(不因高频 `Moved` 而不退出),`Enter` 交既有 `classify`/`process` 处理;提交至多延迟一个 `GRACE`,重绘与 agent 流式不被阻塞

#### Scenario: 孤立回车经续读确认后仍然提交

- **WHEN** 用户手按一次 `Enter`(该批次里唯一的文本内容键,`modifiers=NONE`),`drain` 续读 `poll(GRACE)` 窗口内终端无紧跟事件
- **THEN** 续读确认无续批后走既有提交路径:trim 后非空则整段作为 prompt 提交、清空缓冲、入历史(手敲后 10ms 内无终端续投,续读不改变提交结果,仅多等一个 `GRACE`)

#### Scenario: Release 事件不计入突发规模

- **WHEN** 用户手按一次 `Enter`,Windows 产生 `[Enter Press, Enter Release]` 两个事件落入同一 batch
- **THEN** 先 `is_key_press` 滤除 Release,批内文本内容键数 `n == 1`,该裸 `Enter` 判为**提交**(而非因 Release 使 n=2 被误判换行)

#### Scenario: 前置守卫消费的键不计入突发规模

- **WHEN** 一批里含被前置守卫消费的键(如 `PageUp`)后紧跟一个裸 `Enter`
- **THEN** `PageUp` 归滚动、不计入文本内容键;`n == 1` → 该 `Enter` 判为**提交**

#### Scenario: 批内带 modifier 的换行键照常换行

- **WHEN** 一批事件里含 `Enter+CONTROL` / `Enter+SHIFT` / `Char('j')+CONTROL`
- **THEN** 这些键按既有 `on_key` 换行分支插入换行,不受"突发 vs 孤立"判定与续读影响(二者只接管落单裸 `Enter`)

#### Scenario: pending_permission 活跃时突发只应答首键

- **WHEN** `pending_permission` 活跃,且一批(≥2 键)突发到达、首键为裸 `Enter`
- **THEN** 首键经 `on_key` 命中权限分支正常应答(Allow),随即**丢弃该批余下键**(一串粘贴 `Enter` 不连答)、**不被降级为换行、不往隐藏缓冲插入杂散 `\n`**

#### Scenario: models_picker 活跃时突发过滤输入不丢失

- **WHEN** `models_picker` 活跃,且一批多个过滤 `Char`(如 "gpt")与导航/选中键同批到达
- **THEN** 每键透传给 `handle_models_picker_key`(`Char`→逐个 `push_filter_char`、`Up/Down`→导航、`Enter`→选中并关闭),**过滤字符全部生效、不被截断丢失**;不套用 burst 换行意图;picker 中途关闭后同批尾随的粘贴裸 `Enter` 被丢弃、不落缓冲/提交

#### Scenario: 软浮层补全不受突发护栏影响

- **WHEN** `command_completion` 浮层活跃,且一批 `Char`/`Backspace` 到达
- **THEN** 逐个改缓冲并重过滤候选(不触发硬模态截断),维持既有 `/` 补全行为

#### Scenario: 退出与中断在批处理中仍即时生效

- **WHEN** 一批事件中出现 `Ctrl+C`(无选区)或运行中的 `Esc`
- **THEN** 既有 `should_exit` / 中断守卫逐键前置生效,命中即退出或中断,不被同批余下事件延迟

#### Scenario: 不依赖 bracketed paste

- **WHEN** 运行于 Windows Terminal(crossterm 不产 `Event::Paste`)
- **THEN** 粘贴处理由 batch 突发启发式 + `drain` 内提交前续读 + 剪贴板校准快路径(命中时)实现,`Event::Paste` 分支维持忽略,`terminal.rs` 不变

### Requirement: 消息排队(running 时提交入 app 层可见队列)

TUI SHALL 在 app 层维护可见的 `pending_queue`:agent 运行中(或队列非空)提交的 `Prompt` MUST 入队而非直发,当前轮以任一终止态收场后依次 pop 进 transcript 处理。

**提交分流**:提交非空、非命令的 `Prompt` 时,TUI SHALL 按 `phase` 与队列状态分流——`phase == Ready` **且 `!has_queue()`** → 走既有直发(push transcript `User` + `input_tx.send` + `Busy` + reset 本轮);**否则**(`phase.is_running()`,或 `phase == Ready` 但 `pending_queue` 非空)→ 进 `pending_queue`,**不** send、**不** push transcript、**不** `reset_turn_token_usage`、**不**改 `iteration`。运行中的**命令**(`/xxx`,单行 `parse_command` 命中)SHALL 仍即时执行、不入队(仅 `Prompt` 入队)。

**队列动作(纯 app 状态、可单测)**:`enqueue_prompt(s)` 追加队尾;`dequeue_next() -> Option<String>` 弹出队首,有值时**同时** push transcript `User(该消息)` + 置 `phase=Busy` + `reset_turn_token_usage()`(推进新轮清零上一轮 token),返回该消息供 send;`clear_queue()`;`has_queue()`。

**turn 完成推进**:事件循环 ui_rx 分支处理完 **`TurnComplete` / `Interrupted` / `Error` / `CompactDone`**(终止 / 完成事件之一;**非** `StatusChanged(Idle)`)后,若 `has_queue()`,SHALL 调 `dequeue_next()` 并对返回消息 `input_tx.send(UserInput::Prompt(_))`。`Error` 收场亦推进(否则队列在 provider 报错路径卡死)。**`phase → Ready` 仅由上述终止 / 完成事件驱动;`apply(StatusChanged(Idle))` MUST NOT 置 `phase=Ready`**——否则正常完成路径的 `Idle→TurnComplete` 间会露出 Ready 直发窗口,使陈旧 `TurnComplete` 撞上直发新轮而误推进+错序(第二轮 finding 3)。**channel 恒最多一条**:phase→Ready 仅终止事件驱动(无 Idle 中间窗口),推进(终止事件后 has_queue)与 idle 直发(`!has_queue`)互斥。

**渲染**:`QUEUE_MAX_ROWS = 5`(具名常量),`queue_height = min(pending_queue.len(), QUEUE_MAX_ROWS)`;排队区位于活动行(spinner)与输入框之间,空则零高度、布局同现状;每条渲一行 `⟩ ` 前缀 + 消息**首行**(多行取首行 + `…`),超上限时末行 `⟩ …(+N)`。**`input_content_height_cap` 公式 MUST 减去 `queue_height`**;须保证最小可用屏高(24 行)下 `header3 + 地板8 + gap + activity1 + QUEUE_MAX_ROWS + input_min + status1 + mode1 ≤ screen`。

**取消(两级时间窗)**见「运行中可中断」MODIFIED。**v1 不支持 ↑ 编辑排队消息**(`↑` 维持输入历史/多行光标)。

#### Scenario: 运行中提交入队、不发送、不污染当前轮

- **WHEN** `phase` 运行中(如 `CallingModel`,当前轮已有 token/iteration),提交非空 `Prompt`
- **THEN** 追加 `pending_queue`;**不** send、**不**新增 transcript `User`、当前轮 `iteration` 与 turn token **不被重置**;输入缓冲清空并入输入历史

#### Scenario: Ready 但队列非空时提交仍入队(保 FIFO)

- **WHEN** `phase == Ready` 但 `pending_queue` 非空(`["b"]`),用户提交 `x`
- **THEN** `x` 追加入队(`["b","x"]`)、**不**直发;下一次终止事件推进先 pop `b`

#### Scenario: turn 完成后 pop 队首、reset token、进 transcript 并发送

- **WHEN** `pending_queue=["a","b"]`,事件循环处理到 `TurnComplete`
- **THEN** `dequeue_next()` 弹出 `"a"`、push transcript `User("a")`、置 `phase=Busy`、`reset_turn_token_usage()`(新轮 token 从 0),并 `send(Prompt("a"))`;余 `["b"]`;channel 此刻仅一条

#### Scenario: 运行中 turn 以 Error 收场时队列仍推进

- **WHEN** running 且 `pending_queue=["b"]`,当前轮以 `AgentEvent::Error`(provider 报错/限流/max_iterations)收场
- **THEN** ui_rx 处理 `Error` 后 `has_queue()` 为真 → `dequeue_next()` pop `"b"` 并 send,`"b"` 得处理(不搁浅);后续 idle 提交不插到 `"b"` 之前

#### Scenario: /compact 压缩期间提交入队、CompactDone 推进

- **WHEN** `phase == Compacting`(手动 /compact 进行中)时提交非空 `Prompt`;随后压缩收场发 `CompactDone`
- **THEN** 提交走**入队**而非直发;`CompactDone` 处理后置 `phase=Ready` 并推进 pop 该消息(`dequeue_next` + send),channel 恒最多一条

#### Scenario: 正常完成路径 Idle 不置 Ready、无陈旧 TurnComplete 误推进

- **WHEN** turn A 完成,`run_agent_task` 依次发 `[StatusChanged(Idle), TurnComplete]`;事件循环处理 `Idle` 后、`TurnComplete` 前,用户提交 `x`
- **THEN** 处理 `Idle` **不**置 `phase=Ready`(phase 仍为运行态)→ 用户提交 `x` 走**入队**而非直发;`TurnComplete` 到达才置 Ready 并推进 pop `x`;不出现"陈旧 `TurnComplete` 撞直发新轮 → channel 双 `Prompt` / transcript 错序"

#### Scenario: 运行中命令即时执行不入队

- **WHEN** agent 运行中提交单行 `/clear`(或其它 `parse_command` 命中的命令)
- **THEN** 走既有 `execute_command` 即时执行、**不**进 `pending_queue`

#### Scenario: 队列动作纯逻辑(enqueue/dequeue_next/clear)

- **WHEN** 对 `pending_queue` 依次 `enqueue_prompt("x")` → `enqueue_prompt("y")` → `dequeue_next()` → `clear_queue()`
- **THEN** enqueue 后 `["x","y"]`;`dequeue_next()` 返回 `Some("x")`、push transcript `User("x")`、`phase=Busy`、turn token 归零、余 `["y"]`;`clear_queue()` 后空、`has_queue()` false

#### Scenario: 排队区渲染与高度核算(insta 快照)

- **WHEN** `pending_queue` 含两条(含一条多行),`TestBackend` 渲染;另测超 `QUEUE_MAX_ROWS` 条
- **THEN** 活动行与输入框间出现排队区、各 `⟩ ` 前缀、多行只显首行 + `…`,超上限末行 `⟩ …(+N)`;`input_content_height_cap` 已减 `queue_height`;空队列无排队区(布局同现状),与锁定快照一致

### Requirement: 粘贴折叠占位符(大段粘贴折叠为原子 token)

TUI SHALL 在输入缓冲层把**大段粘贴**折叠为一个原子占位符 token:仅当输入来自粘贴(批级识别)且满足**逻辑行数 `≥ PASTE_FOLD_MIN_LINES`(=15)或字符数 `≥ PASTE_FOLD_MIN_CHARS`(=500)之一**时 MUST 折叠,手打多行与小段粘贴 MUST 照原样逐字符插入。占位符在输入框渲为一行 label,形态**按 chunk 的 `line_count` 分派、与触发原因无关**——多行 chunk(`line_count >= 2`,含行数不达而字符达标者)为 `[Pasted text #N +M lines]`,单行 chunk(`line_count == 1`)为 `[Pasted text #N +K chars]`(K = `chars().count()` 字符数,非字节数)——以 `text_muted` 弱化样式与正文区分;提交时 MUST 原位展开为完整文本。

**触发(批级重建)**:大粘贴命中「剪贴板校准粘贴快路径」时,折叠文本 SHALL 取剪贴板归一化原文、不经本段批级重建(仍经同一 `insert_paste_fold` 入口,存储/渲染/编辑/提交语义不变);其余情形如下。纯函数 `fold_candidate(batch, min_lines, min_chars) -> Option<String>` SHALL **仅当批的 press 键(`press_key_events`)逐键全为文本内容键**(`Char` 非纯 Ctrl + 裸 `Enter`,整批本质是一段纯粘贴)时,重建粘贴文本(`Char`→字符、裸 `Enter`→`\n`)并当 `split('\n').count() >= min_lines` **或** `chars().count() >= min_chars` 时返回 `Some(文本)`;只要出现任一非文本内容键(如 `PageUp`)或两阈值均不达,MUST 返回 `None`。`process_event_batch`(非 `run_tui` 主循环,其顶部持整批 `Vec<Event>`)SHALL 在无 `pending_permission`、无 `models_picker` 且 `fold_candidate` 命中时调 `insert_paste_fold`(`pub(crate)` 入口,封装私有 `apply_input_action`+`refresh_command_completion`)并消费整批,否则走既有逐键路径;paste-guard 跨批续读在上游 `drain_event_batch`,不受影响。

**存储(原子单字符 + 旁挂映射)**:`InputBufferState` SHALL 持 `pasted: BTreeMap<char, PastedChunk>`(`PastedChunk { seq, text, line_count }`)与 `next_paste_seq: u32`。`InsertPasteFold(s)` 动作 MUST 在 `cursor` 处插入一个私有区单字符 sentinel(`char::from_u32(0xE000 + seq)`)、记录 `pasted[sentinel] = { seq, text: s, line_count: s.split('\n').count() }`、`next_paste_seq += 1`。sentinel 为**单个 `char`**,故现有基于 char 边界的光标移动与 `Backspace`/`Delete` 逻辑 MUST NOT 改动即把占位符当**一个原子**(整体跨过、整体删除、光标不入内部)。

**编辑与清理(孤儿裁剪)**:占位符 SHALL 可与手打文字混排、可多个。`prune_pasted()` 的保留集 SHALL 为 **`text` ∪ `draft`** 中出现的 sentinel(历史召回时 chunk 可能仅被 `draft` 引用,text-only 裁剪会使 `↓` 还原后 sentinel 失配丢数据);`Backspace`/`Delete` 删除后、`SetText`(如命令补全)与 `history_up`/`history_down` 替换 `text` 后 MUST `prune_pasted()`。使 draft 还原路径永久不可达的动作 MUST 弃 draft(清空 `draft` 后裁剪),共三处:`exit_history`(召回态打字/编辑退出,仅原 `history_cursor.is_some()` 时生效,非召回态打字零额外开销)、`SetText`(无条件)、`history_down` **还原分支**(Some→None,消费即清空:`text = draft` 后置空 draft——不清则 stale draft 令删除后的 chunk 变 zombie、编号不归零)。裁剪后 `pasted` 为空时 `next_paste_seq` MUST 归零(无存活 chunk,重新编号安全)。显示编号 `#N = seq + 1`(seq 从 0):存活 chunk 期间单调、删除不回收;`pasted` 清空后从 `#1` 重计。

**提交(展开)**:提交时 prompt MUST 取 `expand_folds`(逐 char 把 sentinel 替回 `PastedChunk.text`)后的完整文本;命令旁路判据 MUST 计入 fold(`input().contains('\n') || input_has_fold()` 为真时不试 `parse_command`)。`PushSubmitted` MUST 以展开文本入 history,并清空 `pasted` + 置 `next_paste_seq = 0`(故 `↑` 召回显示展开文本、下一条 `#N` 从 `#1`(seq 0)起)。

**渲染(label dim,跨软换行分段)**:render 端 SHALL 以 `expand_for_display`(sentinel → label,**同时产出各 label 在 display 串中的字节区间**,区间有序、互不重叠、落在 char 边界)+ 光标偏移映射喂 `visual_input_layout`;`InputVisualLayout` SHALL 纯加法暴露 `line_starts`(各可视行在 display 串中的起始字节偏移),不变量:`lines.len() == line_starts.len()` 且 `display[line_starts[i] .. line_starts[i] + lines[i].len()] == lines[i]`。渲染 MUST 以可视行区间与 label 区间**相交**切分 span:label 段渲 `text_muted`、其余正文渲既有 `text_primary`,label 被软换行切开时每一段都 MUST dim;dim 判定 MUST 基于区间而非文本模式匹配(用户手打同款字面文本 MUST NOT 被 dim)。换行与 `input_content_height_cap` 按 label 宽度计入。空 `pasted` 时输入渲染 MUST 与折叠前一致(既有输入快照零 churn);dim 不改文本,既有折叠快照文本亦零 churn。

**Non-Goals(v1)**:不支持 `↑` 编辑/回折已提交的粘贴;不改 `apply_batch_input_key` 既有逐键路径;不保证 `↑↓` 垂直移动跨越/邻接 fold 时光标落位列与屏幕 label 宽度对齐(reduce 按 buffer 列 sentinel=1 算,label ~26 列,v1 接受、真机勿当回归);粘贴文本内**字面 PUA 字符**(U+E000..U+F8FF)与 sentinel 的理论撞车为接受边界(v1 即存在;seq 删空归零使复用窗口略扩,前提同为剪贴板含 PUA,一并接受),不处理。

#### Scenario: 大段粘贴(≥15 行)折叠为占位符 token

- **WHEN** 一批粘贴事件重建出 20 逻辑行文本,`process_event_batch` 时无模态
- **THEN** `fold_candidate` 返回 `Some(该文本)`;施 `InsertPasteFold` 后 `input_line.text` 在光标处含**一个** sentinel、`pasted` 有一项(`line_count=20`)、`next_paste_seq=1`;输入框该处渲为一行 `[Pasted text #1 +20 lines]`,不逐行撑满

#### Scenario: 单行超长粘贴按字符阈值折叠

- **WHEN** 一批粘贴事件重建出**无换行**的 600 字符单行文本(全文本内容键),`process_event_batch` 时无模态
- **THEN** `fold_candidate` 返回 `Some`(`行数 1 < 15` 但 `字符数 600 ≥ 500`);折叠后 `pasted` 一项(`line_count=1`);label 渲为 `[Pasted text #1 +600 chars]`;提交展开为原 600 字符

#### Scenario: 小段粘贴与手打多行不折叠

- **WHEN** 批重建出 14 逻辑行且总字符数 < 500(或手打的多行)
- **THEN** `fold_candidate` 返回 `None`;走既有逐键路径,文本逐字符/逐行进缓冲,`pasted` 为空、渲染同现状

#### Scenario: 折叠触发纯函数(可单测)

- **WHEN** 对 `fold_candidate(batch, 15, 500)` 分别给:14 个裸 `Enter`(+若干 `Char`,重建 15 逻辑行)的纯粘贴批、13 个裸 `Enter`(14 逻辑行、总字符 < 500)批、单行 600 字符批、单行**恰 500** 字符批、单行 499 字符批、14 行 × 40 字符(560)多行批、含 `PageUp` 的混批、空批
- **THEN** 依次:`Some`(行数达标)/ `None`(两阈值均不达)/ `Some`(字符达标)/ `Some`(`≥` 含边界)/ `None` / `Some`(行数不达、字符达标;折叠后 label 仍按 `line_count` 分派为 `+14 lines`)/ `None`(含非文本内容键)/ `None`;边界口径:N 逻辑行 = N−1 个裸 Enter(无尾随)、`count = 裸 Enter 数 + 1`

#### Scenario: 占位符为原子——方向键整体跨过、退格整体删除

- **WHEN** 缓冲为 `a⟦sentinel⟧b`(⟦⟧ 为一个折叠占位符),光标在末尾;先按 `MoveLeft` 两次,再于 sentinel 后按 `Backspace`
- **THEN** `MoveLeft` 一次跨过 `b`、再一次整体跨过占位符(光标落 `a` 后);`Backspace` 于 sentinel 后整体删除该占位符(不进入内部)、随后 `prune_pasted` 使 `pasted` 移除该项

#### Scenario: 删空后编号复位

- **WHEN** 缓冲仅含一个 fold(seq 0),`Backspace` 删除它后再粘贴一段可折叠文本
- **THEN** 删除后 `pasted` 为空且 `next_paste_seq == 0`;新 fold sentinel 复用 U+E000、label 显示 `#1`

#### Scenario: 历史召回往返保留 fold、退出召回弃 draft

- **WHEN** 缓冲含一个 fold,`↑` 召回历史条目后:一路 `↓` 还原(其后再 `Backspace` 删除该 sentinel);另一路直接打字
- **THEN** 还原路:`text` 复原含 sentinel,chunk 完好、label 正常渲染、提交可展开(`prune_pasted` 保留集含 `draft`,召回途中不杀 chunk),且还原**消费** draft(`draft` 为空);随后删除 sentinel → `pasted` 空、`next_paste_seq == 0`(无 stale draft 引用致 zombie);打字路:`exit_history` 清空 `draft`、其独占 chunk 被裁剪、`history_cursor == None`,`text` 中仍存在的 sentinel(若有)不受影响

#### Scenario: SetText 整体替换清孤儿

- **WHEN** `pasted` 持有 chunk(被 `text` 或 `draft` 引用)时发生 `SetText`(如命令补全整体替换输入)
- **THEN** `draft` 被清空、`prune_pasted` 以新 `text` 为准裁剪;新文本不含 sentinel 时 `pasted` 为空且 `next_paste_seq == 0`

#### Scenario: 提交展开为完整文本、history 存展开文本

- **WHEN** 缓冲为 `看这段:⟦#1(20 行原文)⟧`(单 sentinel、无字面 `\n`),按 Enter 提交
- **THEN** prompt = `看这段:` + 20 行原文(`expand_folds`);因含 fold **不**试 `parse_command`;transcript/history 收展开全文;提交后 `pasted` 空、`next_paste_seq=0`;`↑` 召回该条显示展开文本

#### Scenario: 混排多占位符按位置展开保序

- **WHEN** 缓冲为 `⟦#1⟧ 中间 ⟦#2⟧`,两占位符各自原文 A、B
- **THEN** `expand_folds` 得 `A 中间 B`(保序、各归各位);渲染为两个 label 按位置保序(各自形态按其 `line_count` 分派:多行 `+M lines`、单行 `+K chars`)

#### Scenario: line_starts 不变量(可单测)

- **WHEN** 对多逻辑行、软换行(宽度触发)、CJK 宽字符折行、空逻辑行、行恰满(cursor 行末溢出空行)诸 case 调 `visual_input_layout`
- **THEN** 均满足 `lines.len() == line_starts.len()` 且逐行 `text[line_starts[i] .. line_starts[i] + lines[i].len()] == lines[i]`(空行对空串平凡成立);既有 `lines`/`cursor` 断言零改动

#### Scenario: label dim 分段上色(跨软换行,带色断言)

- **WHEN** 输入为 `正文 + fold + 正文` 混排,视口宽使 label 软换行为两段;另有用户**手打**字面 `[Pasted text #1 +2 lines]` 的对照输入
- **THEN** label 两段所在 cell 的 fg 均为 `text_muted`,前后正文 cell 为 `text_primary`(按 `TestBackend` buffer cell 断言,主题无关按 token 比对);手打字面文本 cell 保持 `text_primary`(dim 判定基于 label 字节区间,非文本匹配)

#### Scenario: 折叠渲染与高度核算(insta 快照)

- **WHEN** 输入框含 `前缀文字 [Pasted text #1 +20 lines] 后缀文字`,`TestBackend` 渲染;另测窄宽两宽度与单行 `+K chars` label
- **THEN** 占位符渲为一行 label、与正文可辨;`visual_input_layout` 按 label 宽度换行、`input_content_height_cap` 不因 fold 偷 transcript 地板;空 `pasted` 布局同现状;既有快照零 churn,单行 label 快照新增锁定

### Requirement: Assistant 消息 markdown 渲染(CommonMark+GFM,代码块语法高亮)

TUI SHALL 把 `TranscriptBlock::Assistant(text)` 的正文按 markdown(CommonMark + GFM 表格/删除线)渲染为带样式的 `ratatui::Line`,经 `src/tui/markdown.rs` 的 `render_markdown(text, theme, width) -> Vec<Line<'static>>`;其余 `TranscriptBlock` 变体 MUST NOT 受影响。渲染 MUST 用 `pulldown-cmark` 解析、`syntect`(`default-fancy` feature、纯 Rust regex-fancy 引擎、不引 C `onig`)做围栏代码块语法高亮。

**元素映射**:标题 H1..H6 → `text_title`+`BOLD`(H1 加 `accent_primary`);`**strong**` → `+BOLD`、`*em*` → `+ITALIC`、`~~del~~` → `+CROSSED_OUT`+`text_muted`;`inline code` → `text_body` on `bg_surface_alt`;无序/有序列表 → `• `/`N. ` marker + 按嵌套层缩进;`> quote` → `▎ ` 前缀 + `text_secondary`;`---` → 整行 `─`;链接 `[t](u)` → 文字 `accent_primary`+`UNDERLINED`(URL v1 不内联)。

**代码块**:围栏码块按 info string 首词取语言,`syntect` 逐 token 上色(未知语言退化为纯色),块加区分底色(`bg_sunken`)+ 语言标签;syntect 的 `SyntaxSet`/`ThemeSet` MUST 经 `std::sync::LazyLock`(标准库,不新增第三方)懒加载一次;syntect 主题按 `Theme.is_dark` 选暗/亮;syntect `Color{r,g,b}` 转 ratatui `Color`;`HighlightLines::highlight_line` 返回 `Result` MUST 优雅处理(不 `unwrap`,退 plain)以保半截 markdown 不 panic。

**Theme**:`Theme` SHALL 增 `is_dark: bool` 字段(`midnight()`=`true`、`daylight()`=`false`),供 syntect 选主题。

**行结构**:段落内 `Event::SoftBreak` 与 `Event::HardBreak` MUST 均映射为硬换行(起新逻辑行),**不**按 CommonMark 默认把 SoftBreak 折成空格——以贴合现有 `text.split('\n')` 逐物理行、保纯文本 assistant 行结构不变。相邻块级元素(标题/段落/列表/引用/代码块/表格/hr)之间 SHALL 插入一条空逻辑行分隔,该空行计入 `render_markdown` 产出行数(故 `transcript_line_count`/高度核算随之自洽);此为 **Assistant 单条消息内 markdown 块间**分隔,区别于主 spec 的 `TranscriptBlock` 整块间空行。**注**:CommonMark 会把连续多个空行折成单一段落边界、并 strip 首尾空行,故含多空行/首尾空行的纯文本 assistant 行数可能与 `split('\n')` 现状不同(属预期 churn,不在"逐字节一致"承诺内——该承诺仅限非-assistant 块)。

**换行/截断**:markdown 行内内容 MUST 经 span 感知换行 `wrap_spans(spans: &[Span<'static>], width) -> Vec<Vec<Span<'static>>>` 按 `width` 断行且跨行保留各 span 样式,宽度用 `width::char_width`(CJK 计 2);MUST NOT 改动 `visual_input_layout`(输入框布局)。代码块行与表格单元格**超宽截断 MUST 按 `char_width` 显示宽计**(预留 `…` 显示宽、逐字符累加、**不半切宽字符**),表格填充与截断同用 `display_width` 使含 CJK 列严格对齐。`transcript_lines` 的 `Assistant` 臂 MUST 保留 `◆ ` 首行 marker + 续行缩进,markdown 正文宽度 = 总宽 − marker 宽;`transcript_line_count`/高度核算 MUST 与实际渲染同源(复用 `render_markdown`),不得行数与渲染不一致。

**流式**:`Assistant` 文本流式累积、每帧重解析重渲;半截 markdown(未闭合围栏/强调)MUST 优雅渲染不 panic。选区复制取渲染后文本(markdown 标记隐藏)。

#### Scenario: 行内强调与行内码渲染为带样式 span

- **WHEN** `Assistant` 正文为 `普通 **粗** *斜* 与 `代码``,`render_markdown` 渲染
- **THEN** `粗` 段 span 含 `BOLD`、`斜` 段含 `ITALIC`、`代码` 段 fg=`text_body` 且 bg=`bg_surface_alt`、`普通` 段为 `text_body` 无修饰;标记 `**`/`*`/`` ` `` 不出现在渲染文本中

#### Scenario: 围栏代码块按语言语法高亮

- **WHEN** `Assistant` 正文含 ` ```rust\nfn main() {}\n``` `
- **THEN** 代码块整体加区分底色(`bg_sunken`)、带 `rust` 语言标签;`fn`(关键字)与 `main`(标识符)渲成**不同 fg**(syntect 高亮);`SyntaxSet`/`ThemeSet` 仅懒加载一次

#### Scenario: 未知语言代码块退化为纯色块

- **WHEN** 围栏 info 为未知语言(如 ` ```zzz `)或无语言
- **THEN** 代码块仍加区分底色但不逐 token 上色(纯 `text_body`),不 panic

#### Scenario: 暗/亮主题选不同 syntect 主题

- **WHEN** 同一代码块分别在 `Theme::midnight()`(is_dark=true)与 `daylight()`(is_dark=false)下渲染
- **THEN** 分别选用暗/亮 syntect 主题,代码 token 颜色随之不同;两快照各自锁定

#### Scenario: 列表/引用/标题/hr 渲染

- **WHEN** `Assistant` 正文含 `# 标题`、`- a\n  - b`(嵌套)、`> 引用`、`---`
- **THEN** 标题 `text_title`+BOLD;`a` 一级 `• ` marker、`b` 二级缩进 `• `;引用行 `▎ ` 前缀 + `text_secondary`;hr 为整行 `─`;**且相邻块级元素之间各有一条空逻辑行分隔(计入总行数)**

#### Scenario: 简易 GFM 表格按列宽对齐

- **WHEN** `Assistant` 正文含一个 2 列 GFM 表(含 CJK 单元格)
- **THEN** 表头(`text_title`+BOLD)+ 分隔行(`─`)+ 数据行按各列最大 `display_width` 左对齐;单元格超列宽按 `display_width` 截断 + `…`(预留宽、**不半切宽字符**,填充与截断同用显示宽 → 含 CJK 列各行列位严格对齐);不做单元格内换行

#### Scenario: 仅 Assistant 受影响、其余块不变

- **WHEN** transcript 含 `User`/`Notice`/`Help`/`Error`/`Tool` 各块
- **THEN** 这些块渲染与引入 markdown 前**逐字节一致**(既有非-assistant 快照零 churn);仅 `Assistant` 块走 markdown

#### Scenario: 半截 markdown 流式不 panic

- **WHEN** 流式中途 `Assistant` 文本为未闭合围栏(`` ```rust\nfn ``,无收尾 ` ``` `)
- **THEN** `render_markdown` 尽力渲染(按代码块渲已到达部分)、不 panic;闭合后正常高亮

#### Scenario: 纯文本多行 assistant 每个换行成独立行(SoftBreak 不折空格)

- **WHEN** `Assistant` 纯文本正文为段落内含单 `\n` 的两行(如 `alpha\n你好 beta`,无 markdown 语法)
- **THEN** 每个 `\n`(SoftBreak)仍渲成**独立逻辑行**(续行照 `◆ ` 缩进),**不**被合并为一段加空格;行结构与引入 markdown 前的 `text.split('\n')` 一致

### Requirement: 复制成功轻提示(activity line 右侧)

选区复制**成功**时,TUI SHALL NOT 向 transcript 追加 Notice,改为在 activity line(输入框上方活动指示行)**右侧右对齐**显示一条短暂 hint:「已复制 N 字」(N 为**字符数**非字节数,`text.muted` 样式),存续 `COPY_HINT_TTL = 4s` 后自动消失。过期 MUST 由既有 120ms tick 驱动的无条件重绘承担,MUST NOT 新增定时器;hint 状态 MUST 为纯逻辑可单测(`active_copy_hint(now)` 按 TTL 过滤,渲染侧据此显示)。

左侧活动指示与 hint 并排宽度不足时,hint SHALL 让位(整体跳过渲染),MUST NOT 换行或截断左侧内容。新的成功复制 SHALL 覆盖旧 hint 并重新计时。复制**失败**路径维持既有行为(transcript Notice,见「鼠标拖选与复制」requirement),MUST NOT 受本 requirement 影响。

#### Scenario: 成功复制显示右侧 hint、不入 transcript

- **WHEN** 注入 mock `Clipboard` 成功复制 5 字符
- **THEN** transcript **不**新增任何 Notice;`active_copy_hint(now)` 为「已复制 5 字」;`TestBackend` 渲染 activity line 行右端出现该文案(带色快照锁定,`text.muted`)

#### Scenario: hint 按 TTL 过期、新复制覆盖重计时

- **WHEN** hint 的 `set_at` 为 5s 前(> TTL)时查询 / 渲染;另在 hint 存续期内再次成功复制
- **THEN** 过期后 `active_copy_hint(now)` 为 `None`、渲染不再出现;再次复制后 hint 文本与计时被新值覆盖

#### Scenario: 宽度不足时 hint 让位

- **WHEN** activity line 左侧活动指示 + 间隔 + hint 的总宽超过行宽
- **THEN** 仅渲染左侧活动指示,hint 整体跳过,不换行、不截断左侧

### Requirement: 剪贴板校准粘贴快路径(大粘贴即时折叠与尾流校准丢弃)

TUI SHALL 为大段粘贴提供**剪贴板校准快路径**:事件流仅作「发生了粘贴」的信号,折叠内容取剪贴板原文——命中时 MUST 立即折叠入框(不等 ConPTY 投递完整段),其后仍在到达的事件尾流 MUST 按**期望内容逐事件校准**:匹配即丢弃、失配即转发、内容耗尽即精确清态。未命中时 MUST 回退既有慢路径(合批 + `fold_candidate`),除**一次剪贴板惰性读取与一帧接收提示**外行为与无本 requirement 时一致。

**判定(纯函数,惰性读取,门槛按序短路)**:`try_fast_paste(batch, read_clipboard) -> Option<FastPaste { fold_text, tail }>`,`read_clipboard: FnOnce() -> Result<String, String>` 惰性注入。调用方前置门 = 无 `pending_permission`、无 `models_picker`、`batch.len() >= PASTE_COALESCE_MIN_EVENTS`;函数内按序:① 批内 press 键全为**可重建键**(`Char` 非纯 Ctrl、裸 `Enter`、`Tab`;重建映射 `Char`→字符、`Enter`→`\n`、`Tab`→`\t`)否则 `None`;② 重建文本 `chars().count() >= PASTE_FAST_MIN_MATCH_CHARS`(=8,具名常量)否则 `None`——**此门 MUST 先于 `read_clipboard` 调用**(短打字/IME 短句突发不触发剪贴板读取,以 mock 调用计数可测)。批像粘贴但重建不足 8 字符时,调用方 MUST 先做**预试凑批(top-up)**:以 `PASTE_COALESCE_GRACE` 最多 `PASTE_FAST_TOPUP_ROUNDS`(=5,具名常量)轮 grace 凑批(读入回抽干、非 key 收批,与桥接同规),凑满 ≥8 字符或静默/收批即止,再对累积批**恰一次**尝试;仍未命中 → 累积批交桥接续走慢路,无重复消费(首批常仅数条记录,不凑批则快路径对真实大粘贴永不咬合——真机 1080 行 6s 坐实);③ `read_clipboard()` 失败或全空白 → `None`;剪贴板经**换行归一**(`\r\n`→`\n`、孤 `\r`→`\n`);④ **前缀匹配(先于阈值全扫)**:以 `PasteTailMatcher`(见下)从归一化剪贴板头部逐事件推进批的 press 键,MUST **全部匹配(零转发)**,任一失配 → `None`(非粘贴突发在此快拒,不付全文扫描);⑤ 归一化剪贴板 MUST 满足折叠阈值(`行数 >= PASTE_FOLD_MIN_LINES || 字符数 >= PASTE_FOLD_MIN_CHARS`,与慢路同常量)否则 `None`。命中:`fold_text` = 归一化原文,MUST 经既有 `insert_paste_fold` 入口插入(存储/渲染/编辑/提交语义悉从「粘贴折叠占位符」requirement;行数为 `split('\n')` 口径,尾随换行计入 +1,与该 requirement 一致);`tail` = 已步进到批末匹配位置的 matcher;命中批 MUST NOT 再入 `process_event_batch`(不重复插入)。

**尾流校准丢弃(`PasteTailMatcher`,换行 run 感知纯状态机)**:持归一化剪贴板与匹配游标,事件分四类处置——**非 key 事件** MUST 转发且 MUST NOT 触碰 matcher 状态(滚轮/拖选/焦点/resize 照常,不退出吸收态);**key Release** MUST 丢弃(内容无关,不触碰状态);**非可重建键 press**(纯 Ctrl 组合如 `Ctrl+C`、`Esc`、方向/功能键)MUST 恒转发且 MUST NOT 触碰状态(退出/中断/选区复制**结构性免吞**,内容匹配对其无定义);**可重建键 press**(`Char` 非纯 Ctrl、裸 `Enter`、`Tab`)按期望推进——期望为 `\n` run 起点且事件为裸 `Enter` → 越过整个 run 并进入吸收态、丢弃(吸收态内后续紧邻 `Enter` 丢弃不推进,容「每换行 1 或 2 个 `Enter`」两种到达形态;**可重建的非 `Enter` 键**到达才退出吸收态);期望字符与事件重建字符相等 → 丢弃、推进;失配且期望为**流侧不可靠字符**(astral 码点 `> U+FFFF`、`U+FE0F`、`U+200D`——ConPTY 可能整个吞掉不投递)→ MUST 跳过期望侧连续不可靠字符后对同一事件重试一次(命中 → 丢弃推进,零泄漏;终端真投递 astral 时正常匹配分支先命中不触发跳过);**失配 → 转发该事件**(粘贴后打字 / 模态应答 MUST 经转发直通既有处理),转发后对同一游标继续重试(尾流恢复匹配即恢复丢弃)。**终止三层(吸收先于 done)**:游标达内容末尾**且不在吸收态** → 精确清态(不依赖时钟,慢帧免疫),批内余下事件转发——游标经**尾部 `\n` run** 越至末尾时 MUST 保持吸收态继续吞紧邻 `Enter`(否则源以换行结尾 × 双 `Enter` 形态下吸收残余被转发成 lone Enter 误提交),直到可重建非 `Enter` 键到达(清态并转发该键)或兜底超时;连续失配转发 `>= PASTE_TAIL_ABORT_MISMATCHES`(=16,具名常量;任一匹配将 streak 归零)→ 进入 **aborted 护栏态**(内容跟踪失效,如剪贴板被改写或字符被终端改形):可重建键 MUST 恒丢弃(洪流不入输入框、不再内容比对)、非可重建键与非 key MUST 恒转发(`Esc`/`Ctrl+C`/滚轮直通),`is_done()` MUST 保持 false,由 2s 静默兜底收场(丢弃刷新计时);中止时 debug 模式 SHALL 记 `paste-tail abort streak=16 cursor=N normalized_len=M`(不含内容);key 静默 `>= PASTE_TAIL_QUIET_FALLBACK`(=2s,具名常量;仅匹配丢弃的 key 刷新计时)→ 兜底清态(尾流被截断/尾部吸收态悬置时提示不滞留),判定点 MUST 覆盖「事件批处理前」与「既有 spinner tick」两处。命中时 matcher 已耗尽(整段落入首批,无尾流)则 MUST NOT 置尾流态(提示不空挂)。

**提示**:`paste_tail` 活跃期,活动行右侧 SHALL 显示「⋯ 接收粘贴」轻提示(`text_muted`,复用复制轻提示的右对齐渲染位);**与 copy_hint 同时活跃时 copy_hint 优先**(动作反馈优先,4s TTL 过期后本提示恢复显示;「复制成功轻提示」requirement 不受本 requirement 改写)。快路径**未命中**且批像粘贴(≥ `PASTE_COALESCE_MIN_EVENTS`)时,SHALL 在进入阻塞合批**前**渲染一帧同款提示(阻塞期用户可见状态;该帧为「粘贴突发合并输入」的「整批处理完只渲染一次」的显式例外),批处理完成后随后续帧消失。

**观测与隐私**:剪贴板读取 MUST 仅发生于粘贴样突发判定内;剪贴板内容 MUST NOT 进入任何日志。被快路径消费与尾流**丢弃**的事件(二者不经 `process_event_batch`)SHALL 由 run_tui 层写 `MYSTERIES_TUI_DEBUG_EVENTS` 事件日志(既有 redact 形态不变),行尾分别加 ` disposition=fast-paste` / ` disposition=tail-drop` 标记;尾流**转发**事件 MUST NOT 在 run_tui 层重复记录(由既有 `process_event_batch` 顶部日志自然记录,无标记、夹在 `tail-drop` 行间可辨),同一事件不双行。快路径尝试未命中时 SHALL 记一行 `paste-fast decline reason=<too-short|no-match|clipboard-err|below-threshold> rebuilt_chars=N batch_len=N`;matcher 中止时 SHALL 记一行 `paste-tail abort streak=16 cursor=N normalized_len=M`(均不含任何内容字符,可真机定位)。

**Non-Goals(v1,如实边界)**:模态应答键(可重建 `Char`)恰与尾流期望字符相等时该次被吞(游标推进后经一次失配转发自动重对齐,泄漏字符恒等于用户所敲,自愈;控制键因非可重建而结构性免疫);二次粘贴落在首段尾流未耗尽前 → 其事件大概率失配转发、以普通输入涌入(现象同今日粘贴中再粘,不新增劣化;尾流耗尽后二次粘贴照常);中止(streak 16)护栏态期间用户可重建键与洪流一并被吞(≤ 洪流余量 + 2s 静默,有提示;非可重建键直通);吸收态被批间隙的用户**可重建键**打断(须恰落 CRLF 双 `Enter` 对的 ~8ms 间隙且在 chunk 边界)时残余 `Enter` 失配转发、可能成为换行或 lone Enter 提交——三重小概率,接受并由 disposition 日志可观测;前缀假阳性(≥8 字符突发恰为剪贴板头部,如 IME 整句提交撞上自己刚复制的草稿开头)的**主代价是剪贴板全文被误折入框**——fold 原子、单次 `Backspace` 可删,概率极低,接受;首批不足 `PASTE_COALESCE_MIN_EVENTS`(1 字符级小片)时该字符先按打字入框、其后批因前缀失配退慢路(无提速、不劣化;偏移匹配回删已弃,维持);粘贴样批被剪贴板管理器在 ~ms 窗口内改写且保持前缀一致时 fold 内容以剪贴板为准(「内容取剪贴板」的固有信任边界)。

#### Scenario: 大粘贴命中快路径即时折叠

- **WHEN** 剪贴板持 610 行文本(归一后达折叠阈值),粘贴产生首个 ≥ `PASTE_COALESCE_MIN_EVENTS` 的纯可重建键批,无模态,重建 ≥8 字符且逐事件匹配剪贴板头部
- **THEN** 立即以归一化原文 `insert_paste_fold`(label 行数 = 归一化原文 `split('\n')` 计数),不进入阻塞合批;`paste_tail` 置位、活动行显示「⋯ 接收粘贴」;该批不再入 `process_event_batch`

#### Scenario: 首批不足 8 字符经凑批后命中(top-up)

- **WHEN** 一次大粘贴的首个 `drain` 批仅含 4-15 个事件(重建 2-7 字符,像粘贴但不足匹配门),后续 chunk 以 ~8ms 间隔持续到达
- **THEN** 快路径分支以 grace 凑批(≤5 轮)将累积批凑至 ≥8 字符后尝试**一次**并命中:立即折叠 + 尾流丢弃,总判定延迟 ≤ 5×`PASTE_COALESCE_GRACE`;若凑批期间静默(真·短输入)→ 尝试不命中,累积批交桥接按既有路径处理、无重复消费

#### Scenario: 判定门槛表(纯函数可单测)

- **WHEN** 对 `try_fast_paste` 分别给:含 `PageUp` 批、重建 7 字符批、`read_clipboard` 返回 `Err`/全空白、IME 短句批(与剪贴板头部失配)、归一后 14 行且 499 字符的剪贴板(前缀命中但不达阈值)、命中 case
- **THEN** 依次 `None` / `None`(且 `read_clipboard` **零调用**,mock 计数断言)/ `None` / `None`(前缀快拒,不付全文扫描)/ `None` / `Some`(`fold_text` = 归一化原文、`tail` 游标已在批末)

#### Scenario: 不可靠字符跳过(emoji 内容零泄漏)

- **WHEN** 剪贴板为含国旗 emoji 的配置文本(如 `- name: '🇭🇰 GOMA-HK'`,astral 码点位于行中);流侧分别以「astral 事件被整个吞掉」与「astral 以合成 `Char` 投递」两种形态到达
- **THEN** 吞掉形态:后继字符(空格)到达时 matcher 跳过期望侧 🇭🇰 两码点、重试命中 → 全程 `Drop`、零转发零泄漏、不触发中止;投递形态:astral `Char` 直接匹配期望、不触发跳过;两形态折叠原文均含完整 emoji(保真)

#### Scenario: 换行 run 匹配对到达形态鲁棒

- **WHEN** 剪贴板为 CRLF 源多行文本(含空行);流侧分别以「每换行 1 个 `Enter`」与「每换行 2 个 `Enter`」两种形态投递前缀批与尾流
- **THEN** 两种形态下前缀匹配均命中、尾流均逐事件 `Drop`;折叠文本均为归一化原文(`\n` 换行,空行保真)

#### Scenario: 尾流转发——控制键结构性直通、打字失配转发

- **WHEN** `paste_tail` 活跃期,尾流批之间到达:`Esc`(agent 运行中)、`Ctrl+C`、用户敲的字符、鼠标滚轮批
- **THEN** `Esc`/`Ctrl+C` 为非可重建键 → **恒转发且不触碰 matcher 状态**(中断/退出/选区复制即时生效,结构性免吞——即便期望字符恰为 `c`,`Ctrl+C` 亦不参与内容匹配);用户字符与期望失配 → 转发入框;滚轮为非 key → 转发照常滚动;其后尾流恢复匹配 → 恢复丢弃(resync)

#### Scenario: 尾流期模态应答可达

- **WHEN** agent 运行中粘大段命中快路径,尾流期 agent 发出 `PermissionRequired`(模态经 ui_rx 照常弹出),用户按 `y`
- **THEN** `y` 与期望字符失配(常态)→ 转发 → 权限分支正常应答;若恰与期望字符相等被吞(单次),游标已推进,再按 `y` 失配转发应答成功(自愈,Non-Goal 声明)

#### Scenario: 尾流终止三层(吸收先于 done)

- **WHEN** 分别构造:尾流完整到达且源**不**以换行结尾(游标经字符匹配耗尽)、源**以换行结尾** × 双 `Enter` 形态(游标经尾部 run 越至末尾,紧邻尚有吸收残余 `Enter`,其后用户敲一个字符)、内容分歧(连续 16 次失配转发后洪流继续、夹 `Esc` 与鼠标事件)、尾流被截断后 key 静默 2s(期间仅鼠标事件)
- **THEN** 依次:游标达末尾即清态(批内余下事件转发,不依赖时钟);尾部 run 场景 MUST 保持吸收态吞掉残余 `Enter`(**不**转发、**不**成为 lone Enter 提交),用户字符到达才清态并转发该字符;streak 达 `PASTE_TAIL_ABORT_MISMATCHES` 进入护栏态——其后可重建键(洪流)恒丢弃不入框、`Esc` 与鼠标事件仍恒转发、`is_done()` 为 false,直至 2s 静默兜底清态;`PASTE_TAIL_QUIET_FALLBACK` 兜底清态(经事件批前检查或 spinner tick,提示不滞留)。清态后击键恢复正常入框

#### Scenario: 未命中回退慢路

- **WHEN** 粘贴样批未命中快路径(如剪贴板不达折叠阈值的小段粘贴)
- **THEN** 置一帧「⋯ 接收粘贴」提示后进入既有合批桥接与 `fold_candidate`/逐键路径,处理结果与无快路径时一致;额外代价仅一次剪贴板惰性读取与该帧提示

#### Scenario: 提示与 copy_hint 并存时 copy_hint 优先

- **WHEN** `paste_tail` 活跃期用户拖选并按选区复制键复制成功(`copy_hint` 置位)
- **THEN** 活动行右侧显示「已复制 N 字」(copy_hint 优先);其 TTL 过期后「⋯ 接收粘贴」恢复显示(若尾流仍活跃);复制行为本身不受尾流影响(拖选为鼠标事件转发,复制键为非可重建键、恒转发直通)

#### Scenario: 剪贴板内容不入日志、事件日志不失明

- **WHEN** `MYSTERIES_TUI_DEBUG_EVENTS=1` 下发生快路径命中的粘贴与尾流(含丢弃与转发)
- **THEN** 事件日志维持既有 redact 形态(`Char(<redacted>)`),不出现剪贴板文本片段;命中批与尾流丢弃事件各带 ` disposition=fast-paste` / ` disposition=tail-drop` 标记(run_tui 层记录);尾流转发事件仅由既有批处理日志记录一次(无标记),同一事件不双行

### Requirement: 会话选择 modal

`--resume` 启动 SHALL 弹 `SessionPicker` modal,列出历史会话(短 id / 时间 / 首条 `User` 摘要,mtime 逆序);`Up` / `Down` 移高亮、`Enter` 选中(触发会话 hot-swap,见 `session-persistence` 的 `--resume`)、`Esc` 取消关闭,**其余键被 picker consume**(catch-all,不漏入输入框、不触发退出 / 滚动)。picker 键路由 SHALL 为 **early route**——打开时在事件处理**最前**(于 `press_index += 1` 之后、`should_exit` 之前)吃所有键,先于退出守卫 / 滚动 / selection / queue。

#### Scenario: 导航与选中

- **WHEN** picker 打开,`Up` / `Down` 移动后按 `Enter`
- **THEN** 高亮随之移动,`Enter` 触发选中会话的 hot-swap、picker 关闭

#### Scenario: Esc 取消不退出 app

- **WHEN** picker 打开时按 `Esc`
- **THEN** picker 关闭,不 hot-swap、**不退出 app**(early route 先于 `should_exit`)

#### Scenario: 字符键不漏入输入框

- **WHEN** picker 打开时按普通字符键
- **THEN** 被 picker consume,不进入输入框

### Requirement: 命令类权限框 always-allow 选项

C6 权限框 SHALL 在 `pending_permission` 的 `allow_always_key` 为 `Some` 时,于既有 `[y·允许][n·拒绝]` 间加 `[a·总是允许]` 选项;`allow_always_key` 为 `None`(如 `Edit` 类无 command key)时 MUST NOT 显示该选项(既有 keyless 权限框渲染不变)。按键:`y` → `PermissionReply::AllowOnce`、`n` / `Esc` → `Deny`、`a`(仅 key 存在)→ `AllowAlways`。

#### Scenario: 命令类权限框含 always-allow 且带色快照

- **WHEN** 以带 `command` 参数的 `Execute` 工具触发权限框(`allow_always_key = Some`)渲染
- **THEN** 权限框含 `[y·允许][a·总是允许][n·拒绝]`,与锁定带色快照一致

#### Scenario: 无 key 权限框不含 always-allow

- **WHEN** 以无 `command`(如 `Edit` 的 `path`)的工具触发权限框(`allow_always_key = None`)
- **THEN** 权限框仅含 `[y·允许][n·拒绝]`,既有快照零 churn

#### Scenario: 按 a 回送 AllowAlways

- **WHEN** `allow_always_key = Some` 的权限框活跃时按 `a`
- **THEN** 经 `responder` 回送 `PermissionReply::AllowAlways`

### Requirement: 权限框 diff 按框高截断保动作行可见

C6 权限框的 diff body SHALL 按可用框高截断:以 `area.height` 减去边框与固定行(标题 / tool / args / 动作 / 提示)得 diff 预算,全量 diff 超预算时只显前若干行 + 末行「⋯ 其余 N 行」,确保动作行(`[y·允许]` … `[n·拒绝]`)与提示行**始终完整渲染在框内、不被裁**。既有短 diff(未超预算)MUST NOT 触发截断、渲染不变。

#### Scenario: 长 diff 截断且动作行可见

- **WHEN** 以一个产生超过可用框高的长 diff 的 `write_file` 触发权限框渲染
- **THEN** diff body 截断为末行「⋯ 其余 N 行」,动作行 `[y·允许][n·拒绝]` 与提示行仍完整渲染在框内

#### Scenario: 短 diff 不截断

- **WHEN** 以一个 diff 未超可用框高的工具触发权限框渲染
- **THEN** diff 全量渲染,不出现「⋯ 其余 N 行」,与既有快照一致

### Requirement: Plan 模式指示与 Shift+Tab 达 Plan

Shift+Tab 权限模式轮转 SHALL 纳入 `Plan`(`Normal→AcceptEdits→Yolo→Plan→Normal`);模式指示器 SHALL 为 `Plan` 显示专属 glyph + label(如 `◔ plan`,只读研究态),与既有 Normal/AcceptEdits/Yolo 指示同处、异形。渲染经 `TestBackend`+`insta` 事后快照。

#### Scenario: Shift+Tab 轮转达 Plan

- **WHEN** 当前 `Yolo`,按 Shift+Tab
- **THEN** 当前模式为 `Plan`;再按回到 `Normal`

#### Scenario: Plan 模式指示快照

- **WHEN** 以 `Plan` 模式渲染
- **THEN** 带色快照含 Plan 专属指示(glyph + label),与锁定一致

### Requirement: 共用交互 channel —— plan 审批框与 ask_user 提问框

`AgentEvent` SHALL 扩展承载「工具阻塞待用户结构化输入」的两类请求(仿既有 `PermissionRequired` 的 oneshot 模式):`PlanApprovalRequired{plan, responder: oneshot<PlanDecision>}` 与 `UserQuestionRequired{question, responder: oneshot<Answer>}`。TUI 侧 `PlanApprover` / `UserPrompter` 实现 SHALL 经该 channel 发请求、在 `rx.await` 挂起,渲染对应对话框收结果回送。**批准 plan 时 MUST 翻转共享 `PermissionMode`(`Plan→AcceptEdits`)**——翻转 MUST 在 oneshot 返回**之后**做、勿把 mode mutex 跨 `.await` 持;两 seam MUST 走**同一 `ui_tx`**(即 `select!` 循环所 drain 的那根)。responder 断开 / 取消 → fail-safe(plan → `Reject`、question → 取消 / 空 `Answer`),不 panic。TUI 的 `pending_plan_approval` / `pending_question` 槽 MUST 在 `Interrupted` / `Error` / `TurnComplete` 事件时一并清理(仿既有 `pending_permission`)——防中断时 agent 丢 `rx`、TUI 残留弹框。**提问框选择交互**:SHALL 以 `↑↓` 移动高亮 + 数字键 `1..9` 直跳到第 N 项 + `Enter` 提交(单选 = 当前高亮项、多选 `Space` 切换选中集)+ `Esc` 取消;**MUST NOT 依赖 label 为单字符**(模型常给多字 label,「按 label 字符键选」不可用)。**补充/自定义输入 SHALL 作为可导航的最后一项**(仿 Claude Code 的「Other」):cursor 可移到它、在其上打字即编辑自由文本(带光标)、`Enter` 以该文本为答案提交(`selected` 空、`supplement` = 文本);光标不在该项时打字不误入。**编辑「其它」行时,终端硬件光标 SHALL 定位到该行的输入位置**(仿主输入框 `set_cursor_position`,供 IME / 中文候选窗正确锚定于该行、而非落回主输入框;主输入框那次光标定位在弹框文本域激活时 MUST 让位)。**选项行之间(含「其它」行前)SHALL 留视觉空隙**(不拥挤)。**plan 审批框动作行常驻**:`[y·批准][n·驳回]` + 提示行 SHALL 以底部保留区渲染、**恒可见**——plan 步骤/验收文本换行致内容超框时,步骤区截断(可加「⋯ 其余 N 步」、完整见上方 `submit_plan` 卡),动作行 MUST NOT 被裁出视口(同 permission 框既有防裁策略)。两框渲染经 `TestBackend`+`insta` 事后快照。本机制 MUST NOT 改动 `agent-loop`(经既有 tool + seam 接入)。

#### Scenario: plan 审批框挂起-恢复 + 批准翻模式

- **WHEN** `submit_plan` 触发 `PlanApprovalRequired`,UI 渲染 plan 审批框后回送 `Approve`
- **THEN** approver `approve` 返回 `Approve`,共享 `PermissionMode` 由 `Plan` 翻 `AcceptEdits`;审批框快照含 标题 + 步骤(每步 description + validation)+ `[批准][驳回]`

#### Scenario: ask_user 提问框挂起-恢复 + 选择交互

- **WHEN** `ask_user` 触发 `UserQuestionRequired`,UI 渲染**带编号**选项的提问框;用 `↑↓` / 数字键移动高亮,`Enter` 提交(多选则 `Space` 切换后 `Enter`)
- **THEN** prompter `prompt` 返回对应 `Answer`(所选 label + 补充);提问框快照含 问题 + 编号选项(label + description)+ 高亮标记 + 补充入口;选择**不依赖 label 为单字符**(多字 label 也能选)

#### Scenario: responder 断开 fail-safe

- **WHEN** plan / question 请求发出后 UI 端 responder 被丢弃
- **THEN** plan → `Reject`(安全默认)、question → 取消,`decide`/`prompt` 不 panic

#### Scenario: 中断清理残留弹框

- **WHEN** plan / question 弹框 pending 时收到 `Interrupted`(或 `Error` / `TurnComplete`)
- **THEN** 对应 `pending_plan_approval` / `pending_question` 槽被清、弹框不残留(仿 `pending_permission`)

### Requirement: 执行中的计划进度面板(PlanProgress)

plan 批准后,TUI SHALL 常驻一块「执行中的计划」面板,呈现 plan 各步执行进度与 agent 自检的验收结果;进度经既有 `AgentEvent` 通道以 **fire-and-forget** 上报(非 oneshot 往返),**MUST NOT 改动 `agent-loop`**(经既有 `update_plan` 工具 + `PlanProgressReporter` seam 接入)。

- **事件**:`AgentEvent` SHALL 加 `PlanProgress(PlanProgressUpdate)` 变体(`PlanProgressUpdate {step: usize /*1-based*/, status: StepStatus, validation_result: Option<String>}`);`ChannelProgressReporter{tx}` 实现 `PlanProgressReporter`,`report()` 即 `tx.send(AgentEvent::PlanProgress(..))`(仿 `ChannelObserver` 的 fire-and-forget,`tx` 断开不 panic)。
- **激活**:TUI app state SHALL 持 `current_plan: Option<ActivePlan>`(`ActivePlan{title, steps: Vec<ActiveStep>}`、`ActiveStep{description, validation, status, validation_result}`)。**激活复用 plan 批准时刻**——用户在 plan 审批框点批准、TUI 回送 `PlanDecision::Approve` 的同一处(`answer_pending_plan_approval`,手上 `take()` 出的 request 内含 `plan`),`match decision` **仅 `Approve` 分支**用该 `plan` 建 `ActivePlan`(全步 `Pending`)存入 `current_plan`;`Reject`/`n`/`Esc` **MUST NOT 激活**。**无需新增激活事件**,`ChannelPlanApprover` 不动(仍只负责 oneshot 决策 + 翻 mode)。
- **应用**:收 `PlanProgress` → 按 `step`(1-based)定位对应步。**MUST 先校验 `1 <= step && step <= steps.len()` 再 `steps[step-1]`**——`step==0` 时 `step-1` 是 **usize 下溢**(debug panic / release 越界 panic),与 `step > len` 是**两条独立守卫路径**;改 `status` / 填 `validation_result`。**`step==0`(下溢)/ `step > len`(上界)/ `current_plan==None` SHALL 一律安全忽略、不 panic**。
- **渲染**:`current_plan` 为 `Some` 时,transcript 与输入框之间 SHALL 渲染面板 —— 标题行 + 每步一条。**每步 SHALL 恰好占一个视觉行**(标题行除外):`<glyph> N. <description>`(`Done=✓`/success、`InProgress=▸`/accent、`Pending=○`/muted),**`description` 按面板宽度截断加 `…`、禁止换行**(`Paragraph` **MUST NOT** 开 `Wrap`);**仅当该步 `Done` 且截断后行尾仍有余量**,才追加 dim 的 `validation_result`(同样按剩余宽度截断,不换行、不溢出到第二行)。宽度度量/截断复用既有 display-width 助手;`current_plan==None` 时不渲染该区。**因每步恒一行**,面板高度 = `1 + min(steps, 上限)` 可预测;步骤过多超上限时截断(**保当前 `▸` 步可见**,加「⋯ 其余 N 步」),**MUST NOT 把输入框顶出视口**(高度喂进 `input_content_height_cap`)。完整 plan / 完整 validation 仍可见于 transcript 的 `submit_plan` / `update_plan` 卡。经 `TestBackend`+`insta` 事后带色快照锁定(须含一份**长 description + validation** 的截断态,锁定单行不换行、不溢出)。
- **完成折叠**:当 `current_plan` **所有步骤 `Done`** 且**本轮结束(agent idle,`phase == Ready`)** 时,面板 SHALL 折叠为**单行** `✓ 计划完成 · <title> (<done>/<total>)`(`plan_progress_height` 折叠时为 `1`)。执行中(`Busy` 等非 `Ready`)即使恰好全 `Done` 仍渲染**完整**面板(看得到最后一步 `▸→✓` 的推进);**未全 `Done`** 时不折叠(留完整面板示停在何处,如中断/agent 未跑完)。折叠态仍在下一轮 Prompt 清除。
- **清除**:`current_plan` SHALL 在**新一轮 user turn 真正开始的 choke point** 清空——用户 Prompt 有**两条入口**均须覆盖:① Ready 直发、② 忙时 `enqueue` 后经 `dequeue` 出队再发(建议抽 `begin_user_turn()` helper 统一两处 `push User` 点)。**MUST NOT 在 `enqueue`(排队)时清**(否则误清正在执行的面板);**MUST NOT 挂进 `TurnComplete`/`Interrupted`/`Error` 的 reset 块**(那些清 `pending_*`,但 `current_plan` 要「完成态留屏至下个任务」)。完成态留屏;新 plan 批准覆盖旧的。

#### Scenario: 批准即激活并渲染

- **WHEN** plan 审批框回送 `Approve`(plan 含 N 步)
- **THEN** `current_plan` 为 `Some`、含 N 步全 `Pending`;面板渲染标题 + 各步 `○ N. description`

#### Scenario: PlanProgress 推进步骤与验收

- **WHEN** 已激活 `current_plan`,收 `PlanProgress{step:1, status:Done, validation_result:Some("cargo test → 12 passed")}` 再收 `PlanProgress{step:2, status:InProgress, ..}`
- **THEN** 第 1 步 `✓` 且尾附该验收文本、第 2 步 `▸`、其余 `○`;带色快照与锁定一致

#### Scenario: step==0 下溢安全忽略

- **WHEN** 已激活 `current_plan`,收 `PlanProgress{step:0, ..}`
- **THEN** 不 panic(不得 `step-1` 下溢)、`current_plan` 不变

#### Scenario: 越界 / 无激活计划安全忽略

- **WHEN** `current_plan==None` 时收 `PlanProgress`,或 `step` 上界越界(如 `step:99`,plan 仅 3 步)
- **THEN** 不 panic、`current_plan` 不变、面板不误渲染

#### Scenario: 直发新一轮任务清除面板

- **WHEN** `current_plan` 为 `Some`(某步已 `✓`),Ready 态用户直发新 `Prompt`
- **THEN** `current_plan` 清为 `None`、面板不再渲染

#### Scenario: 出队新一轮任务亦清除面板

- **WHEN** `current_plan` 为 `Some`,一条排队 `Prompt` 经 `dequeue` 出队起新轮
- **THEN** `current_plan` 清为 `None`(出队路径与直发路径同样清)

#### Scenario: 执行中排队不清除面板

- **WHEN** `current_plan` 为 `Some` 且本轮仍在执行,用户提交一条 `Prompt` 进入队列(`enqueue`)
- **THEN** `current_plan` 不变、面板不被误清(清除只发生在新轮真正开始时)

#### Scenario: 面板高度超限不顶出输入框

- **WHEN** `current_plan` 步骤数超出可用高度(如 20 步、矮终端)
- **THEN** 面板截断(保当前 `▸` 步可见,可加「⋯ 其余 N 步」)、输入框仍在视口内、快照锁定该截断态

#### Scenario: 长 description / validation 单行截断不换行

- **WHEN** 某步 `description`(真实模型常给 100+ 字)与其 `validation_result` 合计超过面板宽度
- **THEN** 该步仍**只占一行**、尾部 `…` 截断、**不换行、不溢出到第二行**;不把后续步 / 输入框挤下;快照锁定单行截断态

#### Scenario: 全部完成且 idle 折叠为一行

- **WHEN** `current_plan` 全步 `Done` 且 `phase == Ready`(本轮结束)
- **THEN** 面板渲染**单行** `✓ 计划完成 · <title> (N/N)`、高度 1;下一轮 Prompt 才清

#### Scenario: 执行中即使全 Done 也不折叠

- **WHEN** `current_plan` 全步恰好 `Done` 但 `phase != Ready`(仍在 `Busy`/执行收尾)
- **THEN** 仍渲染完整多步面板(不折叠),待 `phase` 回 `Ready` 后才折叠

### Requirement: 交互工具卡紧凑摘要(submit_plan / update_plan / ask_user)

transcript 工具卡头部的 arg 摘要(`tool_args_preview`)SHALL 为三个交互工具给**紧凑摘要**,**不得 dump 原始 JSON args**(否则长文本/长 validation 一路右溢出屏,如 `submit_plan {"steps":[{"description":"…很长…"}]}`):
- `submit_plan` → `<N> 步`(N = `steps` 数组长度)
- `update_plan` → `step <N> · <status>`(如 `step 1 · done`;**不含 `validation_result` 全文**)
- `ask_user` → question 按显示宽度截断(不含 `options` 全文)

字段缺失/类型不符时回退到既有 `args.to_string()`(不 panic)。

#### Scenario: submit_plan 卡显示步数而非 JSON

- **WHEN** 渲染 `submit_plan` 工具卡(args 为多步 plan)
- **THEN** 摘要为 `N 步`,不含原始 `{"steps":[...]}` JSON

#### Scenario: update_plan 卡显示 step + status 略去 validation

- **WHEN** 渲染 `update_plan` 卡(`{step:1, status:"done", validation_result:"…很长…"}`)
- **THEN** 摘要为 `step 1 · done`,不含 `validation_result` 全文

#### Scenario: ask_user 卡显示截断 question

- **WHEN** 渲染 `ask_user` 卡(question 很长 + 多 options)
- **THEN** 摘要为按显示宽度截断的 question,不含 `options` 全文

### Requirement: 思考过程折叠展示与档位指示

TUI SHALL 通过 `AgentEvent::ThinkingDelta(String)` 承载流式思考文本(`ChannelSink::on_thinking` 发之),`AppState::apply` SHALL 仿 `TextDelta` 累积到 `TranscriptBlock::Thinking(String)`(追加 last_mut 或新建)。`render.rs` 的 `transcript_lines` SHALL 为 `TranscriptBlock::Thinking` **默认展开**渲染:正文上方 SHALL 有一行 header `✻ 思考`(`text_secondary`+`BOLD`,与 `text_muted` 灰字 body 区分);body 为思考正文(每行 `  ` 缩进)。**仅折叠溢出部分**:设 body 渲染后行数为 L、阈值 `THINKING_FOLD_THRESHOLD = 12`(header **不计入** L)——`L ≤ 12` 时 body **全显**、不出折叠标记;`L > 12` 时显示 header + **前 12 行 body** + 一行折叠标记 `… +{M} 行(Ctrl+O 展开)`(`M = L − 12`,`text_muted`);`tools_expanded`(Ctrl+O)为真时 header + body **全显**。MUST NOT 新增独立键位(复用 `tools_expanded`)。footer/状态区 SHALL 显示当前思考档位(仿权限模式指示器)。当模型思考无法关闭(能力 `can_disable:false`)而档位为 `Off` 时,SHALL 出一行提示"该模型思考无法关闭"。TUI 展示走 `TestBackend`+`insta` 事后快照,不走 red-green。

#### Scenario: 思考流式累积成块

- **WHEN** 连续 `ThinkingDelta("思考")` 与 `ThinkingDelta("片段")`
- **THEN** 归入同一 `TranscriptBlock::Thinking`,文本为 `思考片段`

#### Scenario: 短思考默认全显(不折叠)

- **WHEN** Thinking 块 body 渲染后 ≤ 12 行、`tools_expanded=false`
- **THEN** 显示 header `✻ 思考` + 全部灰字正文,无折叠标记

#### Scenario: 长思考默认只折叠溢出尾部

- **WHEN** Thinking 块 body 渲染后为 20 行、`tools_expanded=false`
- **THEN** 显示 header + 前 12 行灰字正文 + 一行 `… +8 行(Ctrl+O 展开)` 折叠标记;快照锁定

#### Scenario: Ctrl+O 展开长思考

- **WHEN** 同一 20 行 body、`tools_expanded=true`
- **THEN** header + 20 行灰字正文全显、无折叠标记;快照锁定

#### Scenario: footer 显示当前档位

- **WHEN** 当前思考档为 `High`
- **THEN** footer/状态区显示当前档位(暗/亮主题各锁快照)

#### Scenario: 恒开模型 Off 提示

- **WHEN** 当前模型 `can_disable:false` 且档位 `Off`
- **THEN** 出一行"该模型思考无法关闭"提示

