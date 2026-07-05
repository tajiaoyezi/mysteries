# Design — add-plan-mode(L1 foundation)

## 目标
research-first 单 agent plan 模式:Plan 期只读调研 → `submit_plan` 交结构化 plan(步骤 + 每步 validation)→ 批准 → 切模式执行。**本 change 只做机制**;validation 强制 / 进度 / 持久化归 enrichment。

## 现状(接地)
- `PermissionMode { Normal, AcceptEdits, Yolo }`(permission/mod.rs:66)+ `auto_allows`(72)+ `cycle_permission_mode`(82,Normal→AcceptEdits→Yolo→Normal)+ `permission_mode_label`(90)+ `gate`(103:ReadOnly→Allow、Edit/Execute→decider)。
- Agent(agent/mod.rs)**每轮发 `registry.schemas()`=全部工具**(139),**无 mode 过滤**;Agent 结构体**不持有 mode**(mode 在 TUI `Arc<Mutex<PermissionMode>>`、共享进 decider)。
- 系统提示 = history 里的 `Message::System`(agent/mod.rs:16),非 Agent 私有。

## 决策

### D1 `PermissionMode::Plan` + cycle 位置
- 加第 4 变体 `Plan`;`permission_mode_label(Plan)="plan"`。
- **cycle 追加到末尾**:`Normal→AcceptEdits→Yolo→Plan→Normal`。理由:保留现有 `Normal→AcceptEdits→Yolo` 肌肉记忆,多按一次 Shift+Tab 达 Plan。**(开放:也可 Plan 置首;语义上 Plan=最克制,但 cycle 会环绕,差别小。)**

### D2 schema-omit 机制:`Tool::plan_only()` + mode-aware 装配
- `Tool` 加 `fn plan_only(&self) -> bool { false }`(默认 false;`submit_plan` override 为 true)。
- schema 按 mode 装配(规则清晰、非按名字 hack):
  - **Plan 模式**:含 `permission_level()==ReadOnly || plan_only()` 的工具 —— 即**只读研究工具 + `submit_plan`**。
  - **非 Plan 模式**:含 `!plan_only()` 的工具 —— 即**全部除 `submit_plan`**(submit_plan 在别的模式无意义)。
- 落点:`ToolRegistry::schemas_for(mode)`(tool-system),Agent 调它取代 `registry.schemas()`。

### D3 Agent 拿到 mode:setter + 轮顶快照
- **审查(实现性)**:Agent 现不持有 mode。**加 setter `set_permission_mode(Arc<Mutex<PermissionMode>>)`(仿 `set_strategy`,字段默认 `Normal`)——不改 `Agent::new` 签名**(17 处调用零改、既有测试逐字节绿;若改签名则全 churn、破坏零回归表述)。共享链已验证:`permission_mode` 在 `tui/mod.rs:109` 建、克隆进 `ChannelDecider` + `state.permission_mode`;装配点再克隆一份经 setter 给 Agent → approver / Shift+Tab 的翻转 Agent 读得到。
- **轮顶快照(审查 HIGH-2)**:每轮 loop 顶 `let mode = *lock`(单次、随即释锁);该轮 `schemas_for(mode)`、指令注入、**及本轮 tool 循环里每一次纵深拒 MUST 复用这同一快照**,勿每 tool_call 重读 mutex——否则同批 `[submit_plan, edit_file]` 里 submit_plan 中途翻 mode 会让 edit_file 静默逃过纵深拒、被 `auto_allows(AcceptEdits,Edit)` 放行。
- 可测:setter 注固定 mode 源,Mock 驱动。headless 默认 `Normal`,行为不变。

### D4 `submit_plan` + `PlanApprover` seam(仿 `PermissionDecider`)
- `Plan { title: String, steps: Vec<PlanStep> }`,`PlanStep { description: String, validation: String }`(serde `Deserialize`,从工具 args 解)。`validation` = 该步**可验收判据**(validation contract 的种子;**强制执行**归 enrichment,本 change 只捕获+呈递)。
- `#[async_trait] trait PlanApprover: Send + Sync { async fn approve(&self, plan: &Plan) -> PlanDecision; }`;`PlanDecision { Approve, Reject(String) }`。real=TUI(展示 plan、收批准/驳回),test=`MockPlanApprover`。
- `submit_plan` 工具持 `Box<dyn PlanApprover>`,`plan_only()=true`、**`permission_level()=ReadOnly`(审查 HIGH-1:呈递审批本质只读;若定 Edit/Execute,Plan 期一调用即被「非只读纵深拒」挡死、plan 永远批不了)**:
  - execute:解析 args→`Plan`(解析失败→is_error)→ `approver.approve(&plan)`;
  - **Approve** → `ToolOutcome{content:"计划已批准,按上述 plan 逐步执行、每步完成后自检其 validation", is_error:false}`;**mode 翻 `Plan→AcceptEdits` 在 approver 实现里做**(TUI 拥有 mode;**翻转在 `rx.await` 返回之后、勿把 mode mutex 跨 await 持**);下一轮 loop 快照读到新 mode → 全工具 schema → 模型按 history 里的 plan 执行。
  - **Reject(reason)** → `ToolOutcome{content:"计划被驳回:{reason};请修改", is_error:true}`(留 Plan、模型改)。
- 可测:`MockPlanApprover`(预置 Approve / Reject),execute 全程离线可测。

### D4b `ask_user` 工具 + 共用交互 seam(与 submit_plan 一起 —— 用户要求)
- **动机**:research-first 撞岔路时别瞎猜——`ask_user` 弹结构化选项让用户定,是 plan 模式最有用的搭档。
- `Question { text, options: Vec<QuestionOption{label, description}>, allow_multi: bool, allow_other: bool }`;`Answer { selected: Vec<String>, supplement: Option<String> }`(serde)。
- `#[async_trait] trait UserPrompter: Send + Sync { async fn prompt(&self, q: &Question) -> Answer; }`;real=TUI(弹 A/B/C+补充框、阻塞收选)、test=`MockPrompter`。
- `AskUserTool` 持 `Box<dyn UserPrompter>`,`permission_level=ReadOnly`、**`plan_only=false`**(任何模式可用;ReadOnly → Plan 期 schema 天然在,供研究澄清)。execute:解析 args→`Question`(失败→is_error)→ `prompter.prompt` → 格式化 `Answer`(含补充)回模型。
- **共用交互 seam**:`ask_user` 与 `submit_plan` 审批本质都是「工具阻塞、等用户结构化输入」。**复用现有 `PermissionDecider` / `tui/channel.rs` 的交互通道模式**:两个 focused trait(`UserPrompter` / `PlanApprover`)各自清晰,共享底层 TUI channel/event 管线(不各造一套);real 实现都在 TUI 侧渲染对应对话框 + 收结果。

### D5 plan 模式系统指令(问就答 / 岔路 ask_user / 任务 submit_plan)
- mode==Plan 时,前置一条 system 指令:「你在 plan 模式(只读:read_file/grep/glob/web_*,**不改文件/不执行命令**)。**用户只是问 → 直接答**;**撞到岔路/歧义 → `ask_user` 弹选项让用户定**;**用户要执行任务 → 调研够了 `submit_plan` 交结构化 plan、每步带可验收 `validation`」。
- **落点(审查 MED-3)**:`msgs = strategy.prepare(history,..)` 返回 Agent **独占的 `Vec<Message>`**;在它与 `ModelRequest` 之间 `msgs.insert(0, System(PLAN_INSTR))`——注入 **transient 请求 messages、非持久 `history`**(否则逐轮累积 + 被存进 session 快照)。mode==Plan 门控、非 Plan 不注入(保 Normal 逐字节等价)。provider 两路都吞多条 System(Anthropic join、OpenAI 逐条)。`forced-final` 段是另一次 prepare(tools 本就空),可不注入。

### D6 纵深拒(Agent 侧、用轮顶快照、双向)
- schema-omit 为主控制;纵深拒为辅——**在 Agent loop 侧判、不改 `gate` 签名**,且 MUST 在 `gate`(agent/mod.rs:186)**之前**(不执行、不弹权限 UI):
  - ① `mode==Plan` 且工具**非 `ReadOnly`** → is_error 拒(封 Plan 期变更;`submit_plan` 是 `ReadOnly`+`plan_only`、**不**被此拒——见 D4)。
  - ② `mode!=Plan` 且工具 `plan_only` → is_error 拒(对称:非 Plan 硬发 `submit_plan` 不该弹审批 + 翻 mode)。
- 二者均用**轮顶快照**(D3);`auto_allows(Plan, Edit|Execute)=false`(即便纵深拒被绕、gate 也不自动放行);`ReadOnly` 由门直放(Plan 期研究工具照跑)。

### D6b assemble_agent 接缝 + headless(审查 ISSUE-1)
- **约束**:`Agent.registry` 私有、`Agent::new` 后无 register 通道 → 两工具须在 `assemble_agent` 内进 registry。而 real impl 需 `ui_tx`(TUI 概念),`assemble_agent` 是 TUI+headless 共用。
- **解**:`assemble_agent` 加 `Option<Box<dyn PlanApprover>>` + `Option<Box<dyn UserPrompter>>`(app.rs 保持抽象、**不漏 `ui_tx`**);**`Some` 才注册**两工具。**TUI 传 `Some`**(channel-backed:持 `ui_tx` + `permission_mode` 克隆),**headless/cli 传 `None`** → plan/ask 工具 **TUI-only(v1)**;headless 默认 Normal、无 Shift+Tab 入口,plan 模式本就是 TUI 特性。
- **零回归红利(审查 ISSUE-2)**:传 `None` 的 registry 无 plan/ask 工具 → `schemas_for(Normal)≡schemas()`;`app.rs:532` 的 `tools.len()==9` 传 `None` **保持 9 不破**(传 `Some` 的新 TUI 测试才 10)。

### D7 批准后目标模式
- 默认 `Plan → AcceptEdits`:plan 已被你批准,执行期 edit 自动过、**execute 仍弹窗**(平衡自动与安全)。**(开放:Normal=每步都弹 / Yolo=全自动。)**

## 接缝
- `src/permission/mod.rs`:`Plan` 变体 + `auto_allows`/`cycle`/`label` 纳入 + gate 纵深。
- `src/tool/mod.rs`:`Tool::plan_only` 默认方法 + `ToolRegistry::schemas_for(mode)`。
- `src/tool/plan.rs`(新):`Plan`/`PlanStep`/`PlanApprover`/`PlanDecision`/`SubmitPlanTool`/`MockPlanApprover`。
- `src/tool/ask.rs`(新):`Question`/`QuestionOption`/`Answer`/`UserPrompter`/`AskUserTool`/`MockPrompter`。
- `src/agent/mod.rs`:`set_permission_mode` setter + 轮顶 mode 快照 + `schemas_for(mode)` + plan 指令注入 transient `msgs` + 双向纵深拒(用快照)。
- `src/app.rs`:`assemble_agent` 加 `Option<Box<dyn PlanApprover>>`+`Option<Box<dyn UserPrompter>>`(`Some` 才注册两工具)+ 经 setter 传 mode 源;**所有调用点**(cli / e2e / ~10 tui 测试传 `None`,TUI 装配传 `Some`)。
- `src/tui/*`:Plan 指示、Shift+Tab 达 Plan、共用交互 channel(仿 permission、**同一 `ui_tx`**)渲染 plan 审批框 + A/B/C 提问框、批准时 mode flip(**`await` 后**)、`pending_plan_approval`/`pending_question` 槽在 中断/错误/完成 清理。

## 风险 / 权衡
- **面大**(跨 5 capability):但每块小且正交;tasks 分段(permission→tool-system→plan 工具→agent 接线→TUI)。
- **mode 翻转在 approver 里做**略隐式:替代是工具 outcome 带 marker、由 app 解释翻转——更显式但更多管线;foundation 取简。
- **web 工具 Plan 期可用**(ReadOnly)= research 需要,但也意味着 Plan 期能联网出站(finding 3 的面还在);Network 级另议时一并处理「Plan 期是否允许 Network 研究」。
- headless(无 TUI)默认 Normal,plan 模式主要经 TUI 使用;CLI 无切换入口(可后续加 flag)。

## 不在本 change
- validation contract **强制**(逐步验判据、失败回滚/重试)、进度跟踪、plan 持久化 → enrichment(第 2 步)。
- web 工具 `Network` 权限级(finding 3)→ 单独。
