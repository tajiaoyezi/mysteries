# Tasks — add-plan-mode(L1 foundation)

红灯纪律:headless 内核(permission / tool-system / agent-loop / 两工具)强制 TDD、红灯独立成步;**红灯停点 = 3.1**(新 trait `PlanApprover`/`UserPrompter` + 新工具接口首次成型)。TUI 外壳按 `TestBackend`+`insta` 事后快照,不走 red-green。执行 agent MUST NOT:git 写、勾第 7 节真机、全仓 `cargo fmt`(只碰你改的)、kill 进程、**加新依赖**(plan/question 结构用现成 `serde`)。

## 1. permission-gate:Plan 模式(纯,强制 TDD)
- [x] 1.1 红→绿:`PermissionMode` 加 `Plan`;`auto_allows(Plan, Edit|Execute)=false`;`cycle_permission_mode` 纳入 `Yolo→Plan→Normal`;`permission_mode_label(Plan)="plan"`。测试:`auto_allows(Plan, *)`、cycle 全环含 Plan、label。

## 2. tool-system:plan_only + schemas_for(纯,强制 TDD)
- [x] 2.1 红→绿:`Tool::plan_only()` default `false`;`ToolRegistry::schemas_for(mode)` —— `Plan`=`只读||plan_only`、非 `Plan`=`!plan_only`,保序。测试:各 mode 过滤 + 保序 + 默认 false;既有 `schemas()` 不变。

## 3. plan / ask 工具(强制 TDD;**红灯停点**)
- [x] 3.1 红(**停点**):`src/tool/plan.rs` —— `Plan`/`PlanStep`/`PlanDecision`/`PlanApprover`(`: Send+Sync`,async)/`SubmitPlanTool`(**`permission_level=ReadOnly`**——否则 Plan 期被纵深拒自我否决、`plan_only=true`、持 `Box<dyn PlanApprover>`)/`MockPlanApprover`;`src/tool/ask.rs` —— `Question`/`QuestionOption`/`Answer`/`UserPrompter`(`: Send+Sync`,async)/`AskUserTool`(`ReadOnly`、`plan_only=false`、持 `Box<dyn UserPrompter>`)/`MockPrompter`。execute 桩返错误 outcome 令断言红。测试(红):`submit_plan` Approve→`is_error=false`、Reject→`is_error`+含理由、非法 args→`is_error`;`ask_user`→含所选+补充、非法 args→`is_error`。**贴测试 + 失败输出,停下等确认。**
- [x] 3.2 绿:两工具 execute 最小实现过 3.1 全测(解析 args→结构→seam→格式化 outcome)。

## 4. agent-loop:接线(强制 TDD)
- [x] 4.1 红→绿:`Agent` 加 **setter** `set_permission_mode(Arc<Mutex<PermissionMode>>)`(仿 `set_strategy`,字段默认 `Normal`、**不改 `Agent::new` 签名**);每轮 **loop 顶读一次 mode 快照**(`let mode = *lock`、随即释锁),本轮 `schemas_for(mode)` 取代 `schemas()`、指令注入、**所有纵深拒全用这个快照**(勿每 tool_call 重读 mutex);`mode==Plan` 把 plan 系统指令(三分支)注入 **`strategy.prepare` 产出的 transient `msgs`**(`msgs.insert(0, System)`、**非 history**);纵深拒双向(Plan+非只读 / 非Plan+plan_only → is_error、不执行、不弹 UI)。测试:Plan 只下发只读+plan_only、注入 transient 指令、`[submit_plan,edit_file]` **快照封中途翻转**、非Plan 硬发 plan_only 被拒、`Normal` **零回归**(既有 agent-loop 测试保持绿——setter 方案下 `Agent::new` 零改)。

## 5. 注册接入
- [x] 5.1 `src/tool/mod.rs` 加 `pub mod plan; pub mod ask;`。`assemble_agent` **加两抽象参数** `Option<Box<dyn PlanApprover>>` + `Option<Box<dyn UserPrompter>>`(app.rs 层保持抽象、**不漏 `ui_tx`**):`Some` 才注册 `SubmitPlanTool`/`AskUserTool`;经 `set_permission_mode` 把共享 mode 源(克隆自 `tui/mod.rs` 建的 Arc)传入 `Agent`。**TUI 装配点传 `Some`**(channel-backed:持 `ui_tx` + `permission_mode` 克隆);**其余所有 `assemble_agent` 调用点(cli.rs / e2e / 约 10 处 tui 测试)传 `None`**(plan 工具 TUI-only v1)。断言:`default_registry_contains_all_builtin_tools` 按最终口径;**`assemble_agent_uses_config_model_and_dispatches_default_tools` 的 `tools.len()`(app.rs:532)传 `None` 时保持 9 不破**(传 `Some` 的新 TUI 测试才 10)。

## 6. TUI 外壳(`insta` 事后,不走 red-green)
- [x] 6.1 Shift+Tab 轮转纳入 `Plan` + Plan 专属指示(render);`AgentEvent` 加 `PlanApprovalRequired` / `UserQuestionRequired`(oneshot,仿 `PermissionRequired`、**走同一 `ui_tx`**);TUI `PlanApprover` / `UserPrompter` 实现(channel 挂起-恢复;**批准翻 `Plan→AcceptEdits` 在 `rx.await` 返回之后、勿把 mode mutex 跨 `.await` 持**;responder 断开 fail-safe);**`pending_plan_approval`/`pending_question` 槽在 `Interrupted`/`Error`/`TurnComplete` 一并清**(仿 `pending_permission`);plan 审批框(标题+步骤+validation+[批准][驳回])+ A/B/C 提问框(问题+选项+补充)渲染。`insta` 带色快照锁定。

## 7. 门禁 + 真机(真机主 agent / 用户;执行 agent 勿勾)
- [x] 7.1 `cargo test --lib` 全绿;`cargo clippy --all-targets -- -D warnings` 零警告;`cargo build`(exe 占用隔离 `CARGO_TARGET_DIR`);`openspec validate add-plan-mode --strict`;`git diff Cargo.toml` **无新依赖**。
- [x] 7.2 真机:Shift+Tab 进 Plan → ① 问问题 → agent 只读调研**直接答**(不 submit_plan);② 给任务 → agent 调研 →(可)`ask_user` 弹选项你选 → `submit_plan` 交结构化 plan → 你**批准** → 切 `AcceptEdits` 执行;验 Plan 期只读约束(不改文件)、两框交互、mode 翻转。
