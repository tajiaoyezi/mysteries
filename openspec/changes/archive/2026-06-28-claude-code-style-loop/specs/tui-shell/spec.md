## ADDED Requirements

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

## MODIFIED Requirements

### Requirement: 状态行常驻 meta

状态行右侧 SHALL 常驻显示 `provider · model · iter X/maxIter · N msgs · cwd`(`设计规范/02` C10),与左侧 phase 并存。`iter` 由 UI 统计当前轮的 `StatusChanged(CallingModel)` 次数得到(新轮 / `TurnComplete` 重置),`msgs` = `transcript` 中**对话块(`User` / `Assistant`)**数 —— 自「单一时间线」合并后,`Tool` 块与命令产出块(Help / Status / Notice)**不计入** `msgs`,保持「消息数」语义;其余取 session 快照(`/model` 切换后 model 同步更新)。

#### Scenario: 状态行 meta 快照

- **WHEN** 给定 session 快照(provider/model/maxIter/cwd)与若干 transcript 块(含 `Tool` 块)渲染
- **THEN** 状态行右侧带色快照含 `provider · model · iter X/maxIter · N msgs · cwd`,其中 `msgs` 只计 `User` / `Assistant` 块(不含 `Tool`),与锁定一致

### Requirement: transcript 滚动

`AppState` SHALL 维护 transcript 的 `scroll_offset`:默认**跟随底部**(新内容自动到底);手动滚动支持 **PageUp / PageDown**(整页)与 **`scroll_up` / `scroll_down`(行级步进 N 行)**;**鼠标滚轮**(`MouseEventKind::ScrollUp` / `ScrollDown`)经行级步进滚动(默认每次 N 行)。滚到非底部时新内容 MUST NOT 强制拉回底部(保持阅读位置);滚回底部时 MUST 恢复跟随;offset MUST clamp 在 [顶, 底](不越界)。**仅 transcript 滚动**,顶栏 / 状态行 / 输入框 / 权限框固定。鼠标滚轮要求终端 guard 进入时启用、退出 / panic 时关闭鼠标捕获。offset / 跟随逻辑 MUST 可单测。

#### Scenario: 跟随、手动滚、clamp(逻辑可测)

- **WHEN** 在底部时追加新内容 → 仍贴底;PageUp 后追加新内容 → 保持当前位置(不回底);PageUp/PageDown 至边界 → offset clamp 不越顶 / 底
- **THEN** `scroll_offset` 按上述规则变化(纯逻辑断言)

#### Scenario: 行级 / 鼠标滚轮步进与触底恢复跟随(逻辑可测)

- **WHEN** 调 `scroll_up`(行级)上滚若干行,再 `scroll_down` 步进直至触底
- **THEN** 上滚后 `follows_bottom` 为假且 offset 按行级步进变化;触底后 `follows_bottom` 恢复为真(后续新内容再次贴底)

#### Scenario: 滚动后的 transcript 快照

- **WHEN** transcript 行数超视口且 `scroll_offset` 指向中段时渲染
- **THEN** 快照只显对应窗口的 transcript 行,顶栏 / 状态行 / 输入框位置不变
