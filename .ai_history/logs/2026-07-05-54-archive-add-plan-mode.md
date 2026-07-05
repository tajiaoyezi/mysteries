# 2026-07-05 · 54 · archive add-plan-mode(L1 plan foundation)

## 决策
- L1 plan 模式 = 强而不肿单 agent(research-first + 结构化 plan + 每步 validation 判据 + 批准即执行)| 选:L1 | 弃:L2 完整 Missions 多 agent 编排、弱内置 plan | 主导:用户拍板 | 依据:对标 Droid Missions / ecc 后收敛
- 拆两步:foundation(机制)+ enrichment(validation 强制 / 进度 / 持久化)| 本 change = foundation | 主导:用户
- schema-omit 靠 `Tool::plan_only()` 谓词(`Plan=ReadOnly||plan_only`、非 `Plan`=`!plan_only`)| 弃:按名 hack | 依据:两轮对抗审查
- **`submit_plan.permission_level=ReadOnly`**(审查 HIGH-1)| 若 Edit/Execute → Plan 期被纵深拒自我否决、plan 永批不了 | 依据:对抗审查 + 真机
- **纵深拒用轮顶 mode 快照、本轮全复用同一局部**(审查 HIGH-2)| 每 tool_call 重读 mutex → 同批 `[submit_plan, edit_file]` 中途翻 mode 致 edit 静默执行 | 依据:审查 + `FlippingPlanApprover` 测试实证
- Agent 拿 mode 用 `set_permission_mode` setter(不改 `Agent::new` 签名)| 17 处调用零改、零回归 | 依据:审查
- plan 指令注入 transient `msgs`(`strategy.prepare` 输出)非持久 `history` | 否则逐轮累积 + 存进 session 快照 | 依据:审查
- `assemble_agent` 加 `Option<Box<dyn PlanApprover/UserPrompter>>`、`Some` 才注册 | TUI 传 `Some`、headless `None`(plan 工具 TUI-only v1)| `app.rs:532` 传 `None` 保 `tools.len()==9` 不破 | 依据:审查
- `ask_user` 与 `submit_plan` 共用 TUI 交互 channel(两 focused trait、仿 `PermissionDecider` oneshot);批准翻 `Plan→AcceptEdits` 在 `rx.await` 后、不跨锁持 mutex | 主导:用户「一起做」 + 审查护栏
- ask_user 选择器 **label-agnostic**(`↑↓`/数字/Enter,非按 label 字符)、「其它」作可导航输入项(仿 Claude Code)、终端光标锚该行(IME 候选)、**弹框 batch 键丢失修复**(`has_pending_dialog` 无条件 `BreakBatch` → 一批只吃第一个键、中文 IME 多字符被丢;改为弹框仍开则 `Continue`)| 主导:8 轮真机迭代 | 依据:真机

## 变更
- permission-gate:MODIFY `PermissionMode`(+`Plan`、`auto_allows`、`cycle`);tool-system:ADD `plan_only` + `schemas_for`;agent-loop:ADD Plan 编排(setter + 快照 + transient 注入 + 双向纵深拒);builtin-tools:ADD `submit_plan` + `ask_user`(Purpose 手改 9→11);tui-shell:ADD Plan 指示 + 共用交互 channel
- 新 `src/tool/{plan,ask}.rs`;`src/tui/channel.rs` 两 channel seam;`render.rs`/`app.rs` 两弹框 + 选择器 + batch 修复
- 无新依赖;`cargo test --lib` 701;真机:Plan 只读约束、ask_user 选/输(中文)、submit_plan 批准翻 AcceptEdits 执行、越界纵深拒

## 待决(enrichment / follow-up)
- enrichment(第 2 步):validation contract 强制逐步验 + 进度跟踪 + plan 持久化
- web 工具 `Network` 权限级(复审 finding 3)—— Plan 期 research 需 web 可用,与 schema-omit 交互另议
- 对抗性 DNS rebinding 残留(web SSRF,见 log 53)
- README / 技术方案「7 个内置工具」→ 11 未同步(change 外)

## 引用
- OpenSpec change:`add-plan-mode`(archived `2026-07-05-add-plan-mode`)
- 跨:add-web-tools(log 52)、add-web-ssrf-guard(log 53)
