# builtin-tools Delta

## ADDED Requirements

### Requirement: submit_plan 提交结构化计划(Plan 模式)

`submit_plan` SHALL 接受结构化 plan `{ title, steps: [{ description, validation }] }`(`validation` = 该步可验收判据);`plan_only()==true`(仅 Plan 模式下发,见 tool-system);**`permission_level()==ReadOnly`**——呈递审批本质是只读动作(真正改动在批准后另起工具);**若定为 `Edit`/`Execute`,Plan 期一调用即被 agent-loop 的「非只读纵深拒」挡掉、approver 永不执行、plan 永远批不了(自我否决)**。经**可注入的 `PlanApprover` seam**(`: Send + Sync`,async)呈递审批,得 `PlanDecision {Approve | Reject(reason)}`:
- **Approve** → `ToolOutcome{content:"计划已批准,按上述 plan 逐步执行、每步完成后自检其 validation", is_error:false}`;权限模式 SHALL 由 approver 实现从 `Plan` 翻至 `AcceptEdits`(翻转在 oneshot 返回**之后**做、勿把 mode mutex 跨 `.await` 持;下一轮全工具可用、按 history 里的 plan 执行)。
- **Reject(reason)** → `ToolOutcome{content 含 reason, is_error:true}`(留 Plan、模型据理由修订)。

args 解析失败(缺 `title` / `steps`)SHALL → is_error、不 panic。审批经 mock approver 可测,不依赖 TUI。

#### Scenario: 批准返回成功

- **WHEN** 注入返 `Approve` 的 mock approver,execute 一个合法 plan
- **THEN** `is_error=false`,content 表已批准(mode 翻转由 approver 实现,契约见 tui-shell)

#### Scenario: 驳回带理由回模型

- **WHEN** 注入返 `Reject("先补测试")` 的 mock approver
- **THEN** `is_error=true`,content 含该理由

#### Scenario: 非法 plan 编码 is_error

- **WHEN** execute 一个缺 `steps` 的 args
- **THEN** `is_error=true`,不 panic

### Requirement: ask_user 向用户提结构化问题

`ask_user` SHALL 接受 `{ question, options: [{label, description}], allow_multi?, allow_other? }`;`permission_level=ReadOnly`、`plan_only()==false`(**任何模式可用**,Plan 期供研究澄清);经**可注入的 `UserPrompter` seam**(`: Send + Sync`,async)弹 A/B/C + 补充框、阻塞取 `Answer {selected, supplement}`,格式化(所选 label + 补充)回模型。args 解析失败(缺 `question`)SHALL → is_error、不 panic。经 mock prompter 可测,不依赖 TUI。

#### Scenario: 返回所选项 + 补充

- **WHEN** 注入返 `Answer{selected:["A"], supplement:Some("再考虑 X")}` 的 mock prompter,execute 一个带选项的问题
- **THEN** `is_error=false`,content 含所选 label 与补充文本

#### Scenario: 非法 args 编码 is_error

- **WHEN** execute 一个缺 `question` 的 args
- **THEN** `is_error=true`,不 panic
