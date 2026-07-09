## ADDED Requirements

### Requirement: resume 恢复的计划进度面板为视觉恢复

`--resume` / `--continue` 还原会话时若得到持久化的 `current_plan`(见 `session-persistence`),TUI SHALL 以既有「执行中的计划进度面板(PlanProgress)」渲染逻辑呈现它——**面板渲染与外观逐字节复用现有实现,不新增视觉变体**。两条还原路径经**统一 plan-only seam `apply_loaded_plan(state, plan)`**(函数体即 `state.current_plan = plan`)写入 `state.current_plan`(`--resume` 为运行时 hot-swap 末尾调用;`--continue` 为启动期经 `SessionStartup` 构造后调用),两路其余会话还原副作用各自处理、不纳入该 seam。还原的 `current_plan` SHALL 遵循既有生命周期:① 用户发出新一轮 user turn(Ready 直发或忙时出队)时,在既有清除 choke point 清空;② 还原时若**全部步骤 `Done` 且 agent 处于 `Ready`**,按既有规则折叠为单行完成态(该态下 `done == total`,渲染 `(<total>/<total>)`),否则渲染完整面板。还原 SHALL 为**纯视觉恢复(只读回显)**:面板在首个新 turn 清空前仅供回看,MUST NOT 重跑任何步骤、MUST NOT 自动重入 Plan 模式、MUST NOT 改动 `agent-loop` 或 `history`(执行续接不在本能力范围)。「不触发步骤重跑」由结构性保证——`UserInput` 枚举无 plan 相关变体、还原路径不改 `agent_history`,非可正面证伪的验收断言。验收经 `TestBackend` + `insta` 事后快照与运行时状态断言。

#### Scenario: 恢复完成态计划折叠

- **WHEN** 还原一个全部步骤 `Done` 的 `current_plan`、agent 处于 `Ready`
- **THEN** 面板以既有单行完成态 `✓ 计划完成 · <title> (<total>/<total>)` 渲染(复用既有 `tui_active_plan_folded` 快照)

#### Scenario: 恢复中断态计划显完整

- **WHEN** 还原一个含未 `Done` 步骤的 `current_plan`
- **THEN** 面板以完整多行形态渲染,各步按 `Done` / `InProgress` / `Pending` 标示、已有 `validation_result` 一并呈现(复用既有 in-progress / 长文截断态快照)

#### Scenario: 恢复的计划在新一轮清除

- **WHEN** 还原 `current_plan` 后,用户发出新一轮 `Prompt`
- **THEN** `current_plan` 按既有 choke point 清空、面板不再渲染
