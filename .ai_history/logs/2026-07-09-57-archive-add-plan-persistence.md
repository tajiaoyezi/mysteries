# 2026-07-09 · 57 · archive add-plan-persistence

## 决策
- **持久化载体 = `SessionLine::Plan(ActivePlan)` 变体** | 选:jsonl 加第 4 类 tagged record | 弃:sidecar 文件(多生命周期)、塞 `history`(与 plan 系统指令 transient·MUST NOT 入 history 冲突)、塞 `transcript` Block(与 submit_plan/update_plan 卡片语义重叠) | 主导:讨论收敛 | 依据:code(session 既有 tagged-line 模型)
- **resume 恢复语义 = 纯视觉恢复,非执行续接** | 选:恢复面板供回看、新 turn 清空、不驱动执行 | 弃:执行续接(需 plan 上下文重建进 history,与现有 agent-loop 冲突,属独立特性) | 主导:用户拍板 | 依据:spec / code
- **接线 seam = plan-only `apply_loaded_plan(state, plan)` 一句赋值** | 选:seam 只设 `current_plan`、picker 与 --continue 两路末尾各调 | 弃:统一整个 hot-swap 的 seam —— `agent_history` 是 `Arc<AsyncMutex>` 须 `.await`、`input_tx`/`session_meta` 纯函数够不着、`--continue` 的 history/transcript 启动期已被 move 消耗、且 `session_meta` 丢失会致 autosave 覆盖写回旧文件 | 主导:两轮对抗审查三方独立收敛 | 依据:code(tui/mod.rs hot-swap 副作用逐行核对)
- **多条 `Plan` 行 → `Err`** | 选:仿 `Meta` 重复报错 | 弃:取最后一条(与 store「异常即报错」哲学不一致) | 主导:二审建议 | 依据:code
- **`load` 返四元组(非具名 `LoadedSession`)** | 选:与现状 3 元组风格一致、diff 最小、`type_complexity` 显式 allow | 弃:`LoadedSession` 结构体(churn 所有调用点解构) | 主导:讨论 | 依据:code
- **TDD = 骨架先行(不计红绿)→ 行为红灯** | 选:类型/签名引入作不计红绿骨架、真红灯落在行为(load 路由 / 多条报错 / 只留最新) | 弃:把「编译失败」当红灯(违反 CLAUDE.md「红灯非编译错」;纯 derive round-trip 无运行期红灯态) | 主导:一审 HIGH + 讨论 | 依据:tests / Rust 类型系统

## 变更
- code:`SessionLine::Plan` 变体;`write` 加 `Option<&ActivePlan>`、`load` 返四元组(多条 Plan→Err);`StepStatus`/`ActivePlan`/`ActiveStep` additive serde(保 `Copy`);`read_session_summary` match 补 `Plan` 忽略臂;`apply_loaded_plan` plan-only seam;`SessionStartup` 加 `plan` 字段;picker hot-swap 末尾 + `run_tui` 构造后两路经 seam 落 `current_plan`;autosave 传 `current_plan`;CHANGELOG 补(含降级不兼容)。
- spec:session-persistence MODIFY 会话快照落盘·会话加载与还原 + ADD plan 进度类型序列化与 resume 恢复;tui-shell ADD resume 视觉恢复面板。
- tests:session 行为红灯(write+load 保 plan / 只留最新 / 多条→Err)转绿 + 兼容/serde/摘要/list_sessions 忽略 Plan 守护;`apply_loaded_plan` 状态断言(Some/None);`prepare_session_startup(Continue)` plan 断言;render §3.6「seam 还原 == 既有折叠快照」对照。

## 待决
- 无。执行续接、web 工具 `Network` 权限级仍属独立后续特性(见 add-plan-mode proposal 的排除项)。

## 引用
- OpenSpec change:`add-plan-persistence`(archived `2026-07-09-add-plan-persistence`);spec:session-persistence、tui-shell。
- 本次 session:propose → **两轮对抗审查(6 只读 agent)+ 两轮修订** → 转发执行 agent 实现(红灯停点 review + 终审读码)的主导判断在本对话。
- 关联决策:add-plan-mode(2026-07-05-54)、add-plan-progress(2026-07-05-55)、add-session-persistence(2026-07-04-49)。
