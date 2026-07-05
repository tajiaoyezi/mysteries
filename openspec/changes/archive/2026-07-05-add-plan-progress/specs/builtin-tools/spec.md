# builtin-tools Delta

## MODIFIED Requirements

### Requirement: submit_plan 提交结构化计划(Plan 模式)

`submit_plan` SHALL 接受结构化 plan `{ title, steps: [{ description, validation }] }`(`validation` = 该步可验收判据);`plan_only()==true`(仅 Plan 模式下发,见 tool-system);**`permission_level()==ReadOnly`**——呈递审批本质是只读动作(真正改动在批准后另起工具);**若定为 `Edit`/`Execute`,Plan 期一调用即被 agent-loop 的「非只读纵深拒」挡掉、approver 永不执行、plan 永远批不了(自我否决)**。经**可注入的 `PlanApprover` seam**(`: Send + Sync`,async)呈递审批,得 `PlanDecision {Approve | Reject(reason)}`:
- **Approve** → `ToolOutcome{content, is_error:false}`,`content` 以「计划已批准」开头并 SHALL 指示 agent 执行期用 `update_plan` 上报进度(每开始一步标记 `in_progress`、每完成一步标记 `done` 并附 validation 自检结果);权限模式 SHALL 由 approver 实现从 `Plan` 翻至 `AcceptEdits`(翻转在 oneshot 返回**之后**做、勿把 mode mutex 跨 `.await` 持;下一轮全工具可用、按 history 里的 plan 执行)。
- **Reject(reason)** → `ToolOutcome{content 含 reason, is_error:true}`(留 Plan、模型据理由修订)。

args 解析失败(缺 `title` / `steps`)SHALL → is_error、不 panic。审批经 mock approver 可测,不依赖 TUI。

#### Scenario: 批准返回成功

- **WHEN** 注入返 `Approve` 的 mock approver,execute 一个合法 plan
- **THEN** `is_error=false`,content 以「计划已批准」开头且含 `update_plan` 进度上报指示(mode 翻转由 approver 实现,契约见 tui-shell)

#### Scenario: 驳回带理由回模型

- **WHEN** 注入返 `Reject("先补测试")` 的 mock approver
- **THEN** `is_error=true`,content 含该理由

#### Scenario: 非法 plan 编码 is_error

- **WHEN** execute 一个缺 `steps` 的 args
- **THEN** `is_error=true`,不 panic

## ADDED Requirements

### Requirement: update_plan 上报计划进度(执行期)

`update_plan` SHALL 接受 `{ step: <1-based 整数>, status: "in_progress"|"done", validation_result?: string }`;`permission_level()==ReadOnly`、**`plan_only()==false`**(执行期用、任何模式可见,不占 schema-omit 的 plan_only 名额)。经**可注入的 `PlanProgressReporter` seam**(`: Send + Sync`)以 **fire-and-forget** 方式(同步 `report(update)`、**不要回值**——区别于 `submit_plan`/`ask_user` 的 oneshot 往返)上报一次 `PlanProgressUpdate {step, status, validation_result}`,随即返回 `ToolOutcome{content:"进度已记录", is_error:false}`。`status` **仅收 `in_progress` / `done`**——`Pending` 是面板激活初始态、不由 agent 上报,故 **`"pending"` 及任何其他值 SHALL 判为非法 → is_error**;实现 **MUST NOT** 直接给三态 `StepStatus` 派生 snake_case `Deserialize`(那样 `"pending"` 会静默反序列化成功、绕过校验),MUST 用独立 2 变体输入枚举(`in_progress`/`done`)或反序列化后显式 reject `Pending`。args 解析失败(缺 `step` / 非法 `status` / **`step==0`**)SHALL → is_error、不 panic(`step` 为 1-based,`0` 非法)。经 mock reporter 可测,不依赖 TUI。面板呈现与越界(含 `step==0` 下溢)忽略契约见 tui-shell。

#### Scenario: done 上报记录进度与验收

- **WHEN** 注入 mock reporter,execute `{step:2, status:"done", validation_result:"cargo test permission → 12 passed"}`
- **THEN** `is_error=false`,content 表进度已记录;reporter 收到一条 `PlanProgressUpdate{step:2, status:Done, validation_result:Some(...)}`

#### Scenario: in_progress 无验收亦合法

- **WHEN** execute `{step:1, status:"in_progress"}`(无 `validation_result`)
- **THEN** `is_error=false`;reporter 收到 `status:InProgress`、`validation_result:None`

#### Scenario: status 为 pending 判非法

- **WHEN** execute `{step:1, status:"pending"}`
- **THEN** `is_error=true`,不 panic(`pending` 不由 agent 上报;实现不得静默接受)

#### Scenario: 非法 args 编码 is_error

- **WHEN** execute 一个缺 `step`、`status` 非法(如 `"bogus"`)、或 `step:0` 的 args
- **THEN** `is_error=true`,不 panic
