# tui-shell Delta

## ADDED Requirements

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
