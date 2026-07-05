# add-plan-progress(L1 收尾,第 2/2 步:进度 + 验收记录)

## Why

`add-plan-mode`(foundation)已交付 L1 四支柱:research-first → 结构化 plan(带每步 validation 判据)→ 批准即执行。但**批准后 plan 直接消失**——执行期看不到「在第几步 / 哪步过了 / validation 自检结果如何」,plan 只活在 message history 里,用户无从跟踪。这是 L1「感觉没完成」的唯一显性缺口。

本 change 补上执行期的**进度可视 + 验收记录**,给 L1 收尾。用户已定档位(三选一里选「进度 + 验收记录」):

- **做**:`current_plan` 常驻状态 + `update_plan` 工具让 agent 逐步标记 + 常驻「执行中的计划」面板(`✓ / ▸ / ○`)+ 每步附 agent **自检的** validation 结果入面板。
- **不做(明确划界,非本 change)**:
  - **持久化 / 落盘 / resume-mid-plan** —— 单条连续 loop 内收益低,YAGNI;若日后要跨会话 resume 再单独立项。
  - **validation 强制执行** —— harness 真跑每步 validation 命令、按退出码 gate、失败回环,面大偏 L2,不塞进 L1;本 change 里 validation 仍是 agent **自检自报**、结构化记录并呈现,不由 harness 执行。

## What Changes

1. **`update_plan` 工具(执行期上报)**:schema = `{ step: <1-based int>, status: "in_progress"|"done", validation_result?: string }`;`permission_level=ReadOnly`、**`plan_only=false`**(执行期用,任何非 Plan 模式可用)。经**可注入的 `PlanProgressReporter` seam**(fire-and-forget,**不要回值**——区别于 `submit_plan`/`ask_user` 的 oneshot)上报一次进度,返回 `ToolOutcome{content:"进度已记录", is_error:false}`。args 解析失败 → is_error、不 panic。
2. **`AgentEvent::PlanProgress`**:新事件变体承载 `{ step, status, validation_result }`;`ChannelProgressReporter`(TUI 侧 `PlanProgressReporter` 实现)`report()` 即发此事件(仿 `ChannelObserver` 的 fire-and-forget,非 oneshot 往返)。
3. **`current_plan` 常驻状态 + 激活时机**:TUI app state 加 `current_plan: Option<ActivePlan>`(`ActivePlan{title, steps:[ActiveStep{description, validation, status, validation_result}]}`、`StepStatus{Pending|InProgress|Done}`)。**激活复用批准那一刻**——用户在 plan 审批框点批准、TUI 回送 `Approve` 的同一处,用手上已有的 `plan` 建 `ActivePlan`(全步 `Pending`),**无需新增激活事件**。
4. **进度应用**:TUI 收 `PlanProgress` → 按 `step`(1-based)定位 `current_plan.steps` 改 `status` / 填 `validation_result`;越界或 `current_plan==None` 时**安全忽略**(不 panic)。
5. **常驻面板渲染**:transcript 与输入框之间渲染「执行中的计划」面板 —— 标题 + 每步 `✓/▸/○ N. description`,已完成步骤后附 dim 的 validation 自检结果。宽度度量复用既有 display-width 助手、超宽截断。`TestBackend`+`insta` 事后快照。
6. **清除时机**:`current_plan` 在**下一次用户 `Prompt`(新一轮任务)提交时**清空(让全 `✓` 的完成态留屏至下个任务);新 plan 批准则覆盖旧的。
7. **`submit_plan` 批准 kickoff 文案微调**:Approve 的 `ToolOutcome.content` 由「…每步完成后自检其 validation」扩为「…每开始一步先 `update_plan` 标记 in_progress、每完成一步 `update_plan` 标记 done 并附 validation 自检结果」——把面板驱动指令交给 agent。前缀「计划已批准」不变(`plan.rs:159` 的 `contains("计划已批准")` 断言不破),**但 `agent/mod.rs:~1525` 有一条对整串的 `assert_eq!`(非 `contains`)——改文案 MUST 同步更新它,否则必红**。

## Impact

- 修改 capability:
  - `builtin-tools`:**MODIFY** `submit_plan`(仅 Approve kickoff 文案) + **ADD** `update_plan`(+ `PlanProgressReporter` seam 契约)。
  - `tui-shell`:**ADD** —— `AgentEvent::PlanProgress` + `ChannelProgressReporter` + `current_plan` 状态 + 批准即激活 + 常驻进度面板渲染 + 越界/空安全忽略 + 下轮清除。
  - `agent-loop`:**无 delta**。`update_plan` 是 ReadOnly 非 plan_only 工具,走既有 loop,无纵深拒交互;seam 于装配注入(同既有「本机制 MUST NOT 改动 agent-loop」范式)。
- Affected code:`src/tool/plan.rs`(新增 `update_plan` / `PlanProgressReporter` / `PlanProgressUpdate` / `StepStatus`;`MockPlanProgressReporter`;submit_plan 文案 + **同步改 `src/agent/mod.rs:~1525` 的整串 exact 断言**)、`src/tool/mod.rs`(注册)、**`src/app.rs`(`assemble_agent` 定义在 `app.rs:158`,加第 **6** 参 / 第 **3** 个 `Option<Box<dyn PlanProgressReporter>>`,**追加在 `user_prompter` 之后**——当前签名已含 `plan_approver`/`user_prompter` 两个 Option;`Some` 才注册 `update_plan`)**、`src/tui/channel.rs`(`AgentEvent::PlanProgress` + `ChannelProgressReporter`)、`src/tui/{app,render}.rs`(`current_plan` 状态 + 激活 + 应用 + 面板渲染 + 清除)、`src/tui/mod.rs`(**状态行硬编码工具数 `+2`→`+3`**,mod.rs:158)。
- **装配 Some/None**:TUI 装配点传 `Some(ChannelProgressReporter)`(→ 注册 `update_plan`,TUI 交互工具增至 3);**其余所有 ~15 个 `assemble_agent` 调用点传 `None`**(headless/cli/测试无进度面板;编译器强制全改)。`None` 路径 **driven `tools.len()`(`app.rs:547`)保持 9 不破**;registry 未过滤成员由 11 → **12**(含 `update_plan`);**注意 driven 计数**:Normal 下 `submit_plan` 被 `plan_only` 省略,故 Some 路径 driven 为 **11**(非 12),勿据 12 写 driven 断言。builtin-tools Purpose 手改:**「11 个内置工具」→「12」、「2 个交互工具」→「3 个」、名单加 `update_plan`**。
- **无新依赖**(update / status 结构用现成 `serde`)。
- 回退:纯增一个只读工具 + 一个事件 + 一块只读渲染;agent 不调 `update_plan` 则面板停在 `Pending`、不影响执行(仅退化为无进度显示)。

## 待定(请你审设计时拍板,见 design.md)
- `update_plan.status` 是否需要 `pending` 回退值(默认:只收 `in_progress`/`done`,`Pending` 仅为初始态、不由 agent 上报)。
- 进度面板位置(默认:transcript 与输入框之间,活动状态行一带)。
- 清除时机(默认:下一次用户 Prompt 提交时清;完成态留屏至下个任务)。
