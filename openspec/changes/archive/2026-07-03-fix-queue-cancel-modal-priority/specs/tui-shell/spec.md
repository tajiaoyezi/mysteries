## MODIFIED Requirements

### Requirement: 运行中可中断(Esc 中断本轮)

`tui` 的 `UserInput` SHALL 增 `Interrupt` 变体、`AgentEvent` SHALL 增 `Interrupted` 变体。`run_agent_task` 的 `Prompt` 分支 MUST 以 `tokio::select!` 把本轮 `Agent::run_observed` 与一个**独立**中断信号并置:中断信号 MUST NOT 复用 `input_rx`。中断到达即 drop 本轮 run future,向 UI 发 `AgentEvent::Interrupted`、状态回 `Idle`,**agent task 与程序均存活**;中断后 MUST NOT 再次调用 provider。**中断路径 MUST 只发 `Interrupted`、不再紧跟冗余的 `StatusChanged(Idle)`**;且 **`apply(StatusChanged(Idle))` MUST NOT 置 `phase=Ready`**——`phase→Ready` 统一由三终止事件(`TurnComplete`/`Interrupted`/`Error`)驱动,消除 Idle 制造的闪帧、`new_message_count` 误增与直发窗口(`bump_new_message_count` 相应移到终止事件分支)。

UI 端 Esc / `Ctrl+C` 按**分流**(仅 `KeyEventKind::Press`,**模态优先于选区**):`pending_permission` 存在 → Esc 拒绝授权 / `Ctrl+C` 维持原行为(**最高优先**);否则存在选区 → Esc 清除选区 / `Ctrl+C` 复制并保留选区;否则**硬模态 `models_picker` 或软浮层 `command_completion` 活跃 → 取消排队分流 MUST NOT 接管**,Esc / `Ctrl+C` 归既有模态/浮层路由(picker 自消费、补全浮层 Esc 关闭),不投 `Interrupt`、不记 `last_cancel_at`、不清排队,与无排队时行为一致;否则**存在排队(`pending_queue` 非空)→ 两级取消(时间窗)**:以 `last_cancel_at` 计 `gap`——`gap >= CANCEL_DOUBLE_TAP`(默认 600ms;第 1 次或非连按)→ 投 `Interrupt` 中断当前轮 + 记 `last_cancel_at=now`(随后 `Interrupted` 触发推进 pop 下一条,即"中断当前+发下一个"),`gap < CANCEL_DOUBLE_TAP`(**快速连按**)→ `clear_queue()` 清空所有排队 + 投 `Interrupt`;否则本轮运行中(**无排队**)→ 投 `Interrupt`;否则就绪 → 退出程序。取消判定 SHALL 抽纯函数 `cancel_action(gap, threshold) -> {InterruptAndAdvance, ClearAll}`(`gap>=threshold`→前者),`Instant` 只在事件循环算 `gap`;**推进 MUST NOT 触碰 `last_cancel_at`**(时间窗不被推进影响,故快速连按可达清空、隔久单按判第 1 次不误清)。**排队由 app 层 `pending_queue` 持有,channel 恒最多一条**。优先级:pending > 选区 > 硬模态/软浮层(`models_picker`/`command_completion`) > 有排队两级取消 > 运行中中断(无排队) > 就绪退出。

#### Scenario: 运行中中断以 Interrupted 收场且不再调用 provider

- **WHEN** 以 Mock provider(在 `complete` 中挂起的脚本)驱动 `run_agent_task`,投入 `Prompt` 后再投 `Interrupt`
- **THEN** 本轮以 `AgentEvent::Interrupted` 收场、状态回 `Idle`,provider 不被再次调用,agent task 继续存活;中断路径只发 `Interrupted`(不再紧跟冗余 `StatusChanged(Idle)`,**Interrupted 后短窗内无任何尾随事件,须有断言锁定**)

#### Scenario: 中断不消费排队的 Prompt

- **WHEN** 中断信号经独立通道到达,而 `input_rx` 中另有已 send 的 `Prompt`(推进发出、尚未取)
- **THEN** 仅本轮被中断;`input_rx` 中的 `Prompt` 不被中断臂吞掉,后续正常消费

#### Scenario: 两级取消时间窗(快速连按清空 / 隔久单按判第 1 次)

- **WHEN** running 且 `pending_queue=["b","c"]`:① 第 1 次 Esc → 中断当前 + 记 `last_cancel_at`(推进 pop `b`);② `gap < 600ms` 内紧接再按 Esc;③ 另测:第 1 次 Esc 后隔 `gap >= 600ms` 才再按 Esc
- **THEN** ②(快速连按)→ `clear_queue()` 清空所有排队 + `Interrupt`;③(隔久单按)→ 判"第 1 次"(中断当前 + 推进下一条),**不**清空排队;推进不改 `last_cancel_at`,故清空档在快速连按下可达、跨轮隔久不误清

#### Scenario: cancel_action 纯函数判定(可单测)

- **WHEN** 对 `cancel_action(gap, threshold)` 分别给 `gap >= threshold`、`gap < threshold`
- **THEN** 分别返回 `InterruptAndAdvance`(中断+推进)、`ClearAll`(清空);判定不触碰 `Instant`,仅比较 `Duration`

#### Scenario: Esc 分流(模态优先于选区,含取消排队)

- **WHEN** 分别在「pending + 有选区」/「有选区、无 pending」/「有排队、运行中、gap≥阈值」/「有排队、gap<阈值」/「本轮运行中、无排队」/「就绪、无排队」下收到 Esc(Press)
- **THEN** 依次:回送 `Deny`(pending 优先)/ 清选区(消费)/ 投 `Interrupt` + 记 last_cancel_at(第 1 次取消,推进下一条)/ `clear_queue()` + 投 `Interrupt`(快速连按清空)/ 投 `Interrupt`(无排队中断)/ `should_exit`(退出);优先级 pending > 选区 > 硬模态/软浮层 > 有排队两级取消 > 运行中中断 > 就绪退出

#### Scenario: 有排队时浮层的 Esc 不被取消排队劫持

- **WHEN** running 且 `pending_queue` 非空,分别在 `models_picker` 打开、`command_completion` 浮层活跃时按 Esc(Press)
- **THEN** Esc 归 picker / 补全浮层(关闭浮层),**不**投 `Interrupt`、**不**记 `last_cancel_at`、**不**清排队;取消排队分流仅在无浮层时接管;推进闸门(三终止事件 + `has_queue` → `dequeue_next` + `send(Prompt)`)SHALL 抽为可测函数并有集成测试锁定(TurnComplete/Error/Interrupted 各推进恰一条、channel 恒最多一条、`StatusChanged(Idle)` 不推进且 Idle 窗口提交入队不直发)

#### Scenario: 中断态渲染为非致命 notice

- **WHEN** UI 收到 `AgentEvent::Interrupted` 后渲染
- **THEN** transcript 末尾含一条「⊘ 已中断本轮」notice 块(`info.fg` / 非致命,区别于 C7 致命错误框),与锁定带色快照一致
