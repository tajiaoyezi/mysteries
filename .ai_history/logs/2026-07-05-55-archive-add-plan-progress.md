# 2026-07-05 · 55 · archive add-plan-progress(L1 收尾:进度 + 验收记录)

## 决策
- L1 enrichment = **进度可视 + 验收记录**(Scope B)| 选:Scope B | 弃:仅进度面板(validation 退化为纯指令)、validation 强制执行(harness 真跑命令 gate,面大偏 L2)、持久化/resume(单 loop 内 YAGNI)| 主导:用户三选一拍板 | 依据:讨论收敛
- 进度上报走 **fire-and-forget seam** `PlanProgressReporter`(同步无返回,仿 `ChannelObserver`)+ `AgentEvent::PlanProgress` | 弃:复用 `submit_plan`/`ask_user` 的 oneshot(进度无需回值)、`Arc<Mutex<Option<ActivePlan>>>` 共享态(多一套跨边界锁)| 依据:三读只读对抗审查 + code
- **激活复用批准时刻**(`answer_pending_plan_approval` 就地建 `current_plan`、零新事件、仅 `Approve` 分支)、**agent-loop 零 delta**(update_plan=ReadOnly 非 plan_only,走既有 loop)| 依据:审查核对真实代码
- `update_plan` args `status` 用**独立 2 变体 `ReportedStatus`** 解析 | 防三态 `StepStatus` 派 snake_case `Deserialize` 静默吞 `"pending"`(审查 F4)| 依据:对抗审查
- `step==0` **双守卫**(tool 侧 + apply 侧,`1<=step<=len` 再 `step-1`)| 防 usize 下溢 panic(审查 F1,原 scenario 只测 step:99 漏 0)| 依据:审查
- 清除挂 `begin_user_turn()` **覆盖直发 + dequeue 两路**、**不** enqueue、**不** reset 块 | 防误清运行中 / 漏清出队新轮(审查 F2)| 依据:审查
- 三轮真机 polish:①每步**单行截断**(去 `Wrap` + `width::truncate_text_to_width`,长中文 description 撑爆修复)②完成**折叠一行**(`all Done && phase==Ready`,`✓ 计划完成 · title (N/N)`)③**交互工具卡紧凑摘要**(submit_plan=`N 步`/update_plan=`step N · status`/ask_user=截断 question,替代 dump 原始 JSON 右溢)| 主导:用户真机逐条反馈 | 依据:真机
- `max_iterations` 改**用户 config 40→200**(非 repo 默认 50)| 多步 plan 每步多轮易触顶;默认未动(product 决策留后)| 依据:用户 config 覆盖默认

## 变更
- builtin-tools:MODIFY `submit_plan`(Approve kickoff 加 `update_plan` 驱动指示,连带改 `agent/mod.rs:1525` 整串 exact 断言)+ ADD `update_plan`(+ `PlanProgressReporter` seam);Purpose 手改 11→12 工具、2→3 交互工具、加 `update_plan`
- tui-shell:ADD 执行中的计划进度面板(`PlanProgress` 事件 + `ChannelProgressReporter` + `current_plan` 激活/应用/单行截断/完成折叠/清除,`AppState::apply` 补一臂,`assemble_agent` 第 6 参/第 3 Option)+ ADD 交互工具卡紧凑摘要
- 新 `src/tui/width.rs` 扩 `truncate_text_to_width`;`assemble_agent` ~15 调用点接第 3 Option;`tui/mod.rs:158` 工具数 +2→+3
- 无新依赖;`cargo test --lib` 736;真机验:面板单行 / 完成折叠 / iter 200 / 工具卡摘要
- 主 agent 严格 post-review:每个执行阶段隔离 `CARGO_TARGET_DIR` 自跑 + 逐行读关键实现 + 读磁盘真快照(非只信绿)

## 待决
- validation 强制执行(L2)、持久化/resume、**面板脱离 Plan 门槛全模式可用**(YOLO/Normal 文本 plan 也驱动面板,另立 change)、☑/☐ checkbox 字形(用户暂留 `✓`)
- **越界发现**:某执行 agent 违「禁全仓 `cargo fmt`」→ `web.rs`/`ask.rs`/`permission/mod.rs` 被 fmt(纯排版重排、零语义);**未入本提交**,待用户定夺(单独提交 / 还原)。`snake/`(真机测试产物)亦未入库

## 引用
- OpenSpec change:`add-plan-progress`(archived `2026-07-05-add-plan-progress`)
- 跨:add-plan-mode(log 54)——本 change 是 L1 拆两步的第 2 步(foundation → enrichment)
