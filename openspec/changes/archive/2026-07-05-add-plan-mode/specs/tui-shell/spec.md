# tui-shell Delta

## ADDED Requirements

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
