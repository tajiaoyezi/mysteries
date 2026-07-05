# Design — add-plan-progress

## 核心决策:进度上报是 fire-and-forget,不复用 oneshot

`submit_plan` / `ask_user` 走 **oneshot 往返**——工具阻塞、等用户结构化输入、拿回值才继续。进度上报**不需要回值**:agent 说「第 2 步开始了」,面板刷新即可,无人回话。所以:

- **seam** = `trait PlanProgressReporter { fn report(&self, update: PlanProgressUpdate); }` —— **同步、无返回**(仿 `AgentObserver` 而非 `PlanApprover`)。`report` 内部 `tx.send(AgentEvent::PlanProgress(..))` 立即返回,`update_plan` 工具随即回 `is_error:false`。
- **不用** `Arc<Mutex<Option<ActivePlan>>>` 共享可变态跨 agent/render 边界:现有架构里 agent→TUI 的唯一通道就是 `AgentEvent` mpsc(FIFO 保序),多引一套共享锁只会多一处 poison 风险。进度沿用 `AgentEvent` 一条路。

## 激活复用批准时刻,零新事件

plan 审批框本就渲染自 `PlanApprovalRequest.plan`。用户点批准、TUI 回送 `Approve` 的**同一处**,手上已有那个 `plan` ——就地 `self.current_plan = Some(ActivePlan::from(&plan))`(全步 `Pending`)。无需再发一个「PlanActivated」事件绕一圈。`ChannelPlanApprover` 那边**不动**(仍只负责 oneshot + 翻 mode)。

## agent-loop 零改

`update_plan` = `ReadOnly` + `plan_only=false`:
- 走既有 tool 循环,`schemas_for(mode)` 天然放行(ReadOnly 各模式可见);
- 无纵深拒交互(既非「Plan 期非只读」,也非「非 Plan 期 plan_only」);
- reporter 于 `assemble_agent`(**定义在 `src/app.rs:158`**,非 agent/mod.rs)装配注入——当前签名已含 `plan_approver`/`user_prompter` 两个 `Option`,reporter 是**第 6 参 / 第 3 Option、追加在 `user_prompter` 之后**(勿插到位 4 打乱既有顺序)。

与既有「共用交互 channel」需求「本机制 MUST NOT 改动 agent-loop」同范式。新 `AgentEvent::PlanProgress` 变体只需在 `AppState::apply`(`app.rs:1078` 穷尽 `match`,无 `_`)补一臂——编译器强制、仅此一处。

## 数据模型

```
// src/tool/plan.rs(工具侧,进 AgentEvent 的载荷)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StepStatus { Pending, InProgress, Done }
#[derive(Debug, Clone, PartialEq, Eq)]  // 测试断言「reporter 收到对应 update / 载荷相等」需之
struct PlanProgressUpdate { step: usize /*1-based*/, status: StepStatus, validation_result: Option<String> }
trait PlanProgressReporter: Send + Sync { fn report(&self, update: PlanProgressUpdate); }

// src/tui(渲染态聚合)
struct ActivePlan { title: String, steps: Vec<ActiveStep> }
struct ActiveStep { description: String, validation: String, status: StepStatus, validation_result: Option<String> }
```

**status 解析(防静默吞 pending)**:`update_plan` args 的 `status` MUST 用**独立 2 变体输入枚举** `#[serde(rename_all="snake_case")] enum ReportedStatus { InProgress, Done }` 解析,再映射到 `StepStatus`。**不得**直接给 `StepStatus` 派生 snake_case `Deserialize`——那样 `"pending"` 会静默反序列化成 `Pending`、`is_error=false`,违反「仅收 in_progress/done」。`ReportedStatus` 拒 `"pending"`/未知 → serde 报错 → is_error。

**下标下溢守卫**:`step` 为 **1-based**(对齐 submit_plan 步骤编号呈现)。应用时 MUST 先校验 `1 <= step && step <= steps.len()` **再** `steps[step-1]`——**`step==0` 时 `step-1` 是 usize 下溢**(debug panic / release 回绕 `usize::MAX` 越界 panic),故 `step==0` 与 `step > len` 是两条独立守卫路径,均**安全忽略、不 panic**;`current_plan==None` 亦忽略。tool 侧亦 belt-and-suspenders 把 `step==0` 判 is_error。`StepStatus` 3 态:`Pending` 为激活初始态,agent 只上报 `InProgress`/`Done`。

## 面板渲染

transcript 与输入框之间常驻:

```
◑ 执行中的计划 · Add plan mode
  ✓ 1. Wire permission gate     ✓ cargo test permission → 12 passed
  ▸ 2. Add update_plan tool
  ○ 3. Wire progress panel
```

glyph:`Done=✓`(success)、`InProgress=▸`(accent/warning)、`Pending=○`(muted);已完成步附 dim 的 `validation_result`。宽度度量 / 截断复用既有 display-width 助手。`insta` 带色快照锁定。

**高度不顶出输入框**:面板作为**条件行**插入 `layout_rows`(`render.rs`),transcript(`Constraint::Min`)自动被挤、`transcript_viewport_height`(=`layout_rows[..][1].height`)随之收缩、视口保真不破。但主布局用**硬编码行号 + 手工偏移**(`render.rs:42-46`:`queue_row`/`input_row`/`status_row`/`mode_row`)、且既有 queue 行本就是条件行 → 面板条件行叠加 = **4 种索引排列**,插入 MUST 重算全部下游索引与直接访问的 `rows[2]`/`rows[4]`。面板高度 MUST 喂进 `input_content_height_cap`(`render.rs:172-188`,现只吃 `permission_height`),否则输入框被顶出;超高时截断保当前 `▸` 步可见(复用 plan 审批框 `render.rs:199-216` 防裁范式)。审批框(`rows[2]`)与进度面板可**同屏共存**(会话中途重 plan),高度预算须容二者。

## 激活与清除时机(单线程 event loop,无竞态)

- **激活 = 批准那一刻、就地、仅 Approve**:`answer_pending_plan_approval`(`app.rs:1500-1503`)`take()` 出 `pending_plan_approval`,其 `.plan` 在手;`match decision` **仅 `Approve` 分支**建 `current_plan = Some(ActivePlan::from(&request.plan))`(全步 `Pending`),`Reject`/`n`/`Esc` **不激活**。`ChannelPlanApprover` 不动(只 oneshot + 翻 mode)。
- **无乱序窗口**:TUI 主循环单个 `tokio::select!`,key 臂与 `ui_rx` 臂串行互斥。激活在同步 key 处理内**先于** `responder.send(Approve)`;agent 拿到 oneshot 才翻 mode→下轮→调 `update_plan`→发 `PlanProgress`,该事件经 mpsc(FIFO)回主循环 apply 时 `current_plan` 早已 `Some`。
- **清除 = 新一轮 user turn 真正开始的 choke point**:用户 Prompt 有**两条**入口都 `push User`——① Ready 直发(`app.rs:1439-1440`);② 忙时 `enqueue`→终止后 `dequeue_next`(`app.rs:565-570`,`mod.rs:457-459`)出队再发。清除 MUST 覆盖**①②两路**(建议抽 `begin_user_turn()` helper 或在两处 `push User` 点统一),**MUST NOT 挂在 `enqueue`**(否则误清正在跑的面板),**亦 MUST NOT 挂进 `TurnComplete`/`Interrupted`/`Error` 的 reset 块**(`app.rs:1151/1171/1185` 清 `pending_*`,但 `current_plan` 要「完成态留屏至下个任务」)。

## 被否决备选

- **validation 强制执行(harness 真跑命令 gate)**:功能最强但面大、偏 L2、风险高;用户档位未选。本 change validation = agent 自检自报 + 结构化呈现。
- **持久化 current_plan 进 session 快照**:单条连续 loop 内 resume 收益低、YAGNI;不做。
- **共享 `Arc<Mutex<Option<ActivePlan>>>`**:多一套跨边界锁,不如 `AgentEvent` 一条路干净。
- **解析 agent 文本自动推进步骤**:脆、不可测;显式 `update_plan` 工具是干净 seam。

## TDD 边界

- **强制 TDD(headless 内核)**:`update_plan` 工具解析/seam 调用/outcome、`PlanProgressReporter` 契约、`ChannelProgressReporter` 发对事件。红灯停点 = 新 trait + 新工具接口首次成型。
- **insta 事后(TUI 外壳)**:`current_plan` 激活 / `PlanProgress` 应用(含越界忽略)/ 面板渲染 / 清除时机。
