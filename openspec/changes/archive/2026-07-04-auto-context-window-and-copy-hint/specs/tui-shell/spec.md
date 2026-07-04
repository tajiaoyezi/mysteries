# tui-shell Delta

## ADDED Requirements

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

## MODIFIED Requirements

### Requirement: 运行中可中断(Esc 中断本轮)

`tui` 的 `UserInput` SHALL 增 `Interrupt` 变体、`AgentEvent` SHALL 增 `Interrupted` 变体。`run_agent_task` 的 `Prompt` 分支 MUST 以 `tokio::select!` 把本轮 `Agent::run_observed` 与一个**独立**中断信号并置:中断信号 MUST NOT 复用 `input_rx`。中断到达即 drop 本轮 run future,向 UI 发 `AgentEvent::Interrupted`、状态回 `Idle`,**agent task 与程序均存活**;中断后 MUST NOT 再次调用 provider。**中断路径 MUST 只发 `Interrupted`、不再紧跟冗余的 `StatusChanged(Idle)`**;且 **`apply(StatusChanged(Idle))` MUST NOT 置 `phase=Ready`**——`phase→Ready` 统一由终止 / 完成事件(`TurnComplete`/`Interrupted`/`Error`/`CompactDone`)驱动,消除 Idle 制造的闪帧、`new_message_count` 误增与直发窗口(`bump_new_message_count` 相应移到终止事件分支)。

UI 端 Esc / `Ctrl+C` 按**分流**(仅 `KeyEventKind::Press`,**模态优先于选区**):`pending_permission` 存在 → Esc 拒绝授权 / `Ctrl+C` 维持原行为(**最高优先**);否则存在选区 → Esc 清除选区 / `Ctrl+C` 复制并保留选区;否则**硬模态 `models_picker` 或软浮层 `command_completion` 活跃 → 取消排队分流 MUST NOT 接管**,Esc / `Ctrl+C` 归既有模态/浮层路由(picker 自消费、补全浮层 Esc 关闭),不投 `Interrupt`、不记 `last_cancel_at`、不清排队,与无排队时行为一致;否则**存在排队(`pending_queue` 非空)→ 两级取消(时间窗)**:以 `last_cancel_at` 计 `gap`——`gap >= CANCEL_DOUBLE_TAP`(默认 600ms;第 1 次或非连按)→ 投 `Interrupt` 中断当前轮 + 记 `last_cancel_at=now`(随后 `Interrupted` 触发推进 pop 下一条,即"中断当前+发下一个"),`gap < CANCEL_DOUBLE_TAP`(**快速连按**)→ `clear_queue()` 清空所有排队 + 投 `Interrupt`;否则本轮运行中(**无排队**)→ 投 `Interrupt`;否则就绪 → 退出程序。取消判定 SHALL 抽纯函数 `cancel_action(gap, threshold) -> {InterruptAndAdvance, ClearAll}`(`gap>=threshold`→前者),`Instant` 只在事件循环算 `gap`;**推进 MUST NOT 触碰 `last_cancel_at`**(时间窗不被推进影响,故快速连按可达清空、隔久单按判第 1 次不误清)。**排队由 app 层 `pending_queue` 持有,channel 恒最多一条**。优先级:pending > 选区 > 硬模态/软浮层(`models_picker`/`command_completion`) > 有排队两级取消 > 运行中中断(无排队) > 就绪退出。(`Phase::Compacting` 视同运行态入本分流;压缩本身不可中断为 v1 Non-Goal,期间 `Interrupt` 无效果、于下一轮 `Prompt` 前被 drain。)

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
- **THEN** Esc 归 picker / 补全浮层(关闭浮层),**不**投 `Interrupt`、**不**记 `last_cancel_at`、**不**清排队;取消排队分流仅在无浮层时接管;推进闸门(终止 / 完成事件 + `has_queue` → `dequeue_next` + `send(Prompt)`)SHALL 抽为可测函数并有集成测试锁定(TurnComplete/Error/Interrupted/CompactDone 各推进恰一条、channel 恒最多一条、`StatusChanged(Idle)` 不推进且 Idle 窗口提交入队不直发)

#### Scenario: 中断态渲染为非致命 notice

- **WHEN** UI 收到 `AgentEvent::Interrupted` 后渲染
- **THEN** transcript 末尾含一条「⊘ 已中断本轮」notice 块(`info.fg` / 非致命,区别于 C7 致命错误框),与锁定带色快照一致

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
