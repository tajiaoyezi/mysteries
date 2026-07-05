# tui-shell Delta

## MODIFIED Requirements

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

## ADDED Requirements

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
