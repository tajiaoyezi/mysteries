# Tasks — add-plan-progress(L1 收尾:进度 + 验收记录)

红灯纪律:headless 内核(`update_plan` 工具 + `PlanProgressReporter` seam + `ChannelProgressReporter` + `current_plan` 应用/清除**纯 AppState 逻辑**)强制 TDD、红灯独立成步;**红灯停点 = 2.1**(新 trait `PlanProgressReporter` + 新工具 `update_plan` 接口首次成型)。仅 §4.2 `render_active_plan`(纯渲染)走 `TestBackend`+`insta` 事后快照。执行 agent MUST NOT:git 写、勾第 5 节真机、全仓 `cargo fmt`(只碰你改的)、kill 进程、**加新依赖**(update/status 结构用现成 `serde`)。

> 连带改点(编译器多半会逼出来,但先记牢,别漏):`src/agent/mod.rs:~1525` 的整串 `assert_eq!`(kickoff 文案,§1.1)、`src/app.rs:158` 的 `assemble_agent` 签名 + `app.rs:547` 断言 + ~15 处调用点(§3.1)、`src/app.rs:1078` 的 `AppState::apply` 穷尽 match 补一臂(§3.2/§4.1)、`src/tui/mod.rs:158` 状态行工具数 `+2`→`+3`(§3.1)。

## 1. submit_plan kickoff 文案(强制 TDD)
- [x] 1.1 红→绿:`SubmitPlanTool` 的 Approve 分支 `ToolOutcome.content`(`src/tool/plan.rs:~110`)由「计划已批准,按上述 plan 逐步执行、每步完成后自检其 validation」扩为含 `update_plan` 驱动指令(「每开始一步先 `update_plan` 标记 in_progress、每完成一步 `update_plan` 标记 done 并附 validation 自检结果」);**前缀「计划已批准」保持不变**。**连带**:`src/agent/mod.rs:~1521-1528` 的 `agent_plan_mode_snapshot_blocks_edit_after_submit_plan_flips_mode` 用的是**整串 `assert_eq!`**(非 `contains`)——**必须同步更新为新串**,否则必红。测试:既有 `submit_plan_approve_returns_success`(`plan.rs:159`,断言 `contains("计划已批准")`)仍绿;**补一条断言 content 含 `"update_plan"`**;更新后的 agent/mod.rs exact 断言绿。

## 2. update_plan 工具 + PlanProgressReporter seam(强制 TDD;**红灯停点**)
- [x] 2.1 红(**停点**):`src/tool/plan.rs` —— 加:
  - `enum StepStatus { Pending, InProgress, Done }`(`#[derive(Debug, Clone, Copy, PartialEq, Eq)]`);
  - `struct PlanProgressUpdate { step: usize /*1-based*/, status: StepStatus, validation_result: Option<String> }`(`#[derive(Debug, Clone, PartialEq, Eq)]`——测试要比较载荷);
  - `trait PlanProgressReporter: Send + Sync { fn report(&self, update: PlanProgressUpdate); }`(**同步 fire-and-forget、无返回**);
  - `struct UpdatePlanTool { reporter: Box<dyn PlanProgressReporter> }`(`permission_level=ReadOnly`、**`plan_only=false`**);
  - `MockPlanProgressReporter`(`Arc<Mutex<Vec<PlanProgressUpdate>>>` 记录收到的 update 供断言);
  - **status 解析防吞 pending**:args 里 `status` MUST 用**独立 2 变体输入枚举** `#[serde(rename_all="snake_case")] enum ReportedStatus { InProgress, Done }` 解析后映射到 `StepStatus`;**不得**直接给三态 `StepStatus` 派生 snake_case `Deserialize`(会静默接受 `"pending"`)。
  `execute` 桩返错误 outcome 令断言红。测试(红,覆盖正常+失败+边界):
  - `{step:2, status:"done", validation_result:"cargo test → 12 passed"}` → `is_error=false` 且 reporter 收到 `PlanProgressUpdate{step:2, status:Done, validation_result:Some(...)}`;
  - `{step:1, status:"in_progress"}`(无 validation_result)→ `is_error=false`、reporter 收 `status:InProgress`、`validation_result:None`;
  - **`{step:1, status:"pending"}` → `is_error=true`**(不得静默接受);
  - 缺 `step` / `status:"bogus"` / **`step:0`** → `is_error=true`、不 panic;
  - `plan_only()==false`、`permission_level()==ReadOnly`。
  **贴测试 + 失败输出,停下等确认。**
- [x] 2.2 绿:`UpdatePlanTool::execute` 最小实现过 2.1 全测(解析 args→`ReportedStatus`→`PlanProgressUpdate`→`reporter.report`→`ToolOutcome{content:"进度已记录",is_error:false}`;`step==0` 与非法 status 走 is_error)。

## 3. 注册接入 + ChannelProgressReporter(强制 TDD)
- [x] 3.1 红→绿:`assemble_agent`(**`src/app.rs:158`**)**加第 6 参 / 第 3 个 `Option<Box<dyn PlanProgressReporter>>`,追加在 `user_prompter` 之后**(当前签名已含 `plan_approver`/`user_prompter` 两 Option,**勿插到位 4**);`Some` 才注册 `UpdatePlanTool`。**编译器强制改全部 ~15 调用点**:`src/cli.rs:551`、`tests/e2e.rs:97`、`src/app.rs:{530,597,610}`、`src/tui/mod.rs:110`(**生产 TUI 装配,传 `Some(ChannelProgressReporter)`**)、`src/tui/mod.rs` ~10 处 tui 测试——**除 TUI 装配点外一律传 `None`**。**连带**:`src/tui/mod.rs:158` 状态行硬编码 `default_registry().schemas().len() + 2` → **`+ 3`**。测试断言:
  - **`None` 路径 driven `recorded[0].tools.len()`(`app.rs:547`)保持 `9` 不破**;
  - `Some` 路径 **registry 未过滤成员含 `update_plan`、`registry.schemas()` 计数 = `12`**;
  - **勿写 driven `== 12`**:Normal 下 `submit_plan` 被 `plan_only` 省略,Some 路径 driven 为 **11**;若既有某 Some-path driven 计数断言现为 `10`,加 update_plan 后应更新为 `11`。
- [x] 3.2 红→绿:`src/tui/channel.rs` —— `AgentEvent` 加 `PlanProgress(PlanProgressUpdate)` 变体;`ChannelProgressReporter{tx}` 实现 `PlanProgressReporter`,`report()` 即 `let _ = tx.send(AgentEvent::PlanProgress(update))`(fire-and-forget,仿 `ChannelObserver`,**不 oneshot**)。**连带**:`AppState::apply`(`src/app.rs:1078` 穷尽 `match`,无 `_`)编译器强制补 `PlanProgress` 一臂(逻辑见 §4.1)。测试:`report` 后 `rx.try_recv()` 得 `PlanProgress` 且载荷相等;`tx` 断开时 `report` 不 panic。

## 4. TUI:current_plan 激活 / 应用 / 清除(§4.1 强制 TDD 红→绿;§4.2 render 走 insta 事后)
- [x] 4.1 红→绿(**纯 AppState 逻辑、非 insta**):app state 加 `current_plan: Option<ActivePlan>`(`ActivePlan`/`ActiveStep`/复用 `StepStatus`)。
  - **激活(仅 Approve)**:`answer_pending_plan_approval`(`app.rs:1500-1503`)`take()` 出 request、`match decision` **仅 `Approve` 分支**用 `request.plan` 建 `ActivePlan`(全步 `Pending`)存 `current_plan`;`Reject` **不激活**。`ChannelPlanApprover` 不动。
  - **应用**:`AppState::apply` 收 `PlanProgress` → **先校验 `1 <= step && step <= steps.len()` 再 `steps[step-1]`**(改 `status`/填 `validation_result`);**`step==0`(下溢)/ `step > len` / `current_plan==None` 一律安全忽略、不 panic**。
  - **清除**:抽 `begin_user_turn()`(或两处 `push User` 点统一)在**新一轮 user turn 开始**清 `current_plan`——覆盖 ① Ready 直发(`app.rs:1439`)与 ② `dequeue_next` 出队(`app.rs:565`)**两路**;**不在 `enqueue`(`app.rs:1442`)清**;**不在 `TurnComplete`/`Interrupted`/`Error` reset 块(`app.rs:1151/1171/1185`)清**(完成态留屏)。
  测试(单元,红→绿):`Approve` 建全 `Pending`;`Reject` 不激活;`PlanProgress{done}` 改对步 + 填 validation_result;**`step:0` 忽略不 panic**;`step:99` 忽略;`None` 收事件忽略;**Ready 直发 Prompt 清**;**`dequeue` 出队清**;**`enqueue` 排队不清**;`TurnComplete`/`Interrupted`/`Error` **不**清(完成态留屏)。
- [x] 4.2 `render_active_plan`(`insta` 事后):`current_plan==Some` 时在 transcript 与输入框之间渲染面板 —— `◑ 执行中的计划 · <title>` + 每步 `✓/▸/○ N. <description>`(`Done=✓ success`、`InProgress=▸ accent`、`Pending=○ muted`),已完成步后附 dim `validation_result`;`None` 时不渲染。**布局连带(易错)**:面板作**条件行**插入 `layout_rows`;**重算 `render.rs:42-46` 全部下游行号**(`queue_row`/`input_row`/`status_row`/`mode_row`)与直接访问的 `rows[2]`/`rows[4]`(条件行 × 条件 queue = 4 排列);**面板高度喂进 `input_content_height_cap`(`render.rs:172-188`)**,超高时截断保当前 `▸` 步可见(可加「⋯ 其余 N 步」)、**MUST NOT 顶出输入框**(复用 `render.rs:199-216` 防裁范式)。宽度度量/超宽截断复用既有 display-width 助手。`insta` 带色**全帧**快照(须同含面板 + 输入框 + 状态行,验索引不错位):① 一份「执行中」态(混合 ✓/▸/○ + 一条 validation_result);② 一份 overflow 态(步骤数 > 可用高度,验截断 + 输入框仍在)。

## 4b. 真机修正:每步单行截断(§5.2 真机暴露;insta 事后)
- [x] 4.3 `render_active_plan`(`src/tui/render.rs:1319`)每步**恰一个视觉行**:**关掉 `Paragraph` 的 `.wrap(Wrap{..})`**(1420 行);每步先拼 `  {glyph} {n}. ` 前缀,算 display-width,把 `description` 按剩余宽度截断加 `…`;**仅当 `Done` 且截断后仍有余量**才追加 dim `validation_result`(按剩余宽度截断,同行、不溢出)。截断跨多 span 时从尾部逐 span 截,保证整行 ≤ `area.width`。`plan_progress_height`(`render.rs:108`)保持 `1 + steps.len()`(现在准确)。overflow「⋯ 其余 N 步」+ 保 `▸` 步可见逻辑不变(1 行/步后即正确)。**根因**:原实现 wrap 开 + 塞完整 description + 完整 validation → 长文本换行撑爆、`plan_progress_height` 按 1 行/步算致高度失配、Paragraph 裁切不滚。测试:**新增一份长 description(100+ 字)+ 长 validation 的 insta 全帧快照**,锁定该步单行 `…` 截断、不换行、输入框仍在;重生成既有 `tui_active_plan_in_progress`/`tui_active_plan_overflow` 两快照(若因去 wrap 变化)。

## 4c. 真机修正:完成后折叠一行(§5.2 真机反馈;insta 事后)
- [x] 4.4 `render_active_plan` + `plan_progress_height`:当 `current_plan` **全步 `Done` 且 `phase == Ready`**(agent idle/本轮结束;`Phase::Ready` 见 `app.rs:31` enum)时,**折叠**——`plan_progress_height` 返 `1`、`render_active_plan` 只渲一行 `✓ 计划完成 · <title> (<done>/<total>)`(`✓` success、标题 body、计数 muted)。**执行中(`phase != Ready`)即使全 `Done` 仍渲完整面板**;**未全 `Done` 不折叠**。抽 helper 如 `active_plan_is_folded(state)`(`all Done && phase==Ready`)供两处共用,避免 height 与 render 判据分叉。测试:**新增 insta 全帧快照**——全 `Done` + `Ready` 折叠成一行(断言单行 + 输入框上移);另断言执行中(`Busy`)全 `Done` 仍完整(可复用/改 in_progress 快照或加一条)。清除时机不变(下一轮 Prompt 清)。

## 4d. 真机修正:交互工具卡紧凑摘要(§5.2 真机反馈;强制 TDD 纯函数)
- [x] 4.5 红→绿:`tool_args_preview`(`src/tui/render.rs:1054`)加三个 arm——`submit_plan` → `format!("{} 步", steps.len())`;`update_plan` → `format!("step {} · {}", step, status)`(**不含 `validation_result`**);`ask_user` → question 按显示宽度截断(复用 `width::truncate_text_to_width`,如 48 列;**不含 options**)。字段缺失/类型不符回退既有 `args.to_string()`、不 panic。**根因**:三工具落到 `_ => args.to_string()` fallback,dump 完整 JSON → 长文本右溢出屏。`tool_args_preview` 是纯函数 → **强制 TDD**:先写测试(submit_plan 多步→`"N 步"`、update_plan done+长 validation→`"step 1 · done"` 不含 validation、ask_user 长 question→截断且不含 options、缺字段→回退不 panic)跑红,再加 arm 转绿。

## 5. 门禁 + 真机(真机主 agent / 用户;执行 agent 勿勾)
- [x] 5.1 `cargo test --lib` 全绿;`cargo clippy --all-targets -- -D warnings` 零警告;`cargo build`(exe 占用隔离 `CARGO_TARGET_DIR`);`openspec validate add-plan-progress --strict`;`git diff Cargo.toml` **无新依赖**。builtin-tools Purpose(`specs/builtin-tools/spec.md:4`)archive 时手改**三处**:「**11** 个内置工具」→「**12**」、「**2** 个交互工具」→「**3**」、交互工具名单加 `update_plan`。
- [ ] 5.2 真机:Shift+Tab 进 Plan → 给任务 → agent 调研 → `submit_plan` → 批准 → 切 `AcceptEdits` 执行,**观察面板**:每步 `▸`→`✓` 推进、完成步显 validation 自检结果;中途看得到「在第几步」;`enqueue` 一条排队 prompt 时面板不被误清;新任务真正起(直发或出队)后面板清除;长 plan 不顶出输入框。
