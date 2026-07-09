# Design — add-plan-persistence

## Context

Plan 模式(`add-plan-mode` + `add-plan-progress`,均已 archive)把执行进度承载在 TUI 内存态 `current_plan: Option<ActivePlan>`(`src/tui/app.rs:476`):批准时由 `ActivePlan::from_plan` 激活、`update_plan` 逐步推进、新一轮 user turn 清空。它**不进 session 快照**——`src/session/` 对 plan 零持久化,故 `--resume` / `--continue` 还原会话后进度面板丢失。

现状接地(权威 = code / tests):
- `SessionLine`(`src/session/mod.rs:30`)externally-tagged enum:`Meta(SessionMeta)` / `Msg(Message)` / `Block(TranscriptBlock)`。
- `write(&self, meta, history, transcript)` 全量重写整个 jsonl;`load(id) -> io::Result<(SessionMeta, Vec<Message>, Vec<TranscriptBlock>)>` 按 tag 分派、顺序无关、**未知 tag → `Err`**。
- `ActivePlan{title, steps}` / `ActiveStep{description, validation, status: StepStatus, validation_result}`(`src/tui/app.rs`)与 `StepStatus{Pending, InProgress, Done}`(`src/tool/plan.rs`)当前**无 serde derive**(`StepStatus` 现为 `Debug, Clone, Copy, PartialEq, Eq`)。
- 每轮 autosave 落点 `write_session_snapshot`(`src/tui/mod.rs:415-424`,内含 `store.write`),由 `terminal_session_event`(TurnComplete/CompactDone/Interrupted/Error)门控、**每轮末落盘一次**。
- **两条 resume 路径机制不同**:`--resume`→picker→运行时 hot-swap(`mod.rs:335-357`,有可变 `state`);`--continue`→启动期 `prepare_session_startup`→`SessionStartup`(`84-89`,**无 plan 字段**)→`run_tui` 构造 `AppState`(155-168,**不设 `current_plan`**)。

## Goals / Non-Goals

**Goals:**
- `current_plan` 随会话落盘,`--resume` / `--continue` 还原后进度面板视觉重现(完成态折叠、中断态完整)。
- 向后兼容:旧会话(无 plan 记录)照常加载,`current_plan = None`。
- 复用既有 autosave / hot-swap 路径,不新增写触发点或独立加载通道。

**Non-Goals:**
- **执行续接**:resume 后 agent 接着跑未完成步骤(需把 plan 上下文重建进 `history`,与「plan 系统指令 transient、MUST NOT 入 history」冲突,属独立特性)。
- 面板视觉/交互任何改动(渲染逻辑逐字节复用)。
- 前向兼容(旧二进制读新 session)——见 Risks。

## Decisions

### D1 持久化载体 = `SessionLine::Plan(ActivePlan)` 新变体
在既有 tagged-line 模型上加第 4 变体,与 `Meta`/`Msg`/`Block` 同构,`write`/`load` 顺手扩展。
- **备选 A(sidecar 文件 `<id>.plan.json`)**:弃——多一套文件生命周期(创建/清理/与主文件一致性),违背「取简」;单文件 round-trip 测试也更直接。
- **备选 B(把 plan 塞进 `history` 或 `transcript` Block)**:弃——plan 系统指令明确 transient、MUST NOT 入 history;塞 transcript 会与既有 `submit_plan`/`update_plan` 卡片语义重叠且污染单一时间线。`current_plan` 是**独立于对话内容的面板态**,单独一条记录最清晰。

### D2 `write` 加参数 `Option<&ActivePlan>`,搭既有 autosave 顺风车
`write` 签名从 `(meta, history, transcript)` 扩为 `(meta, history, transcript, plan: Option<&ActivePlan>)`;TUI autosave 调用点传 `state.current_plan.as_ref()`。因 autosave 每轮跑,plan 的最新态天然随轮落盘;全量重写模型保证文件内至多一条 `Plan` 行。
- **备选(新方法 `write_with_plan`)**:弃——两个 write 路径易漂移;单一 write 是唯一落盘出口更安全。传 `None` 即退化为原行为,既有非 plan 场景零语义变化。

### D3 `load` 返回四元组 `(.., Option<ActivePlan>)`
`load` 分派时把 `Plan` 行收进第 4 元素 `Option<ActivePlan>`。多条 `Plan` 行 → **`Err`**(仿 `Meta` 重复报错,维持 store「异常即报错」一贯性;全量重写下正常至多一条)。
- **备选(独立 `load_plan(id)` 方法)**:弃——两次读文件、两处解析易不一致;单次 `load` 一趟解析所有 tag 最省。
- **备选(具名结构体 `LoadedSession{meta,history,transcript,plan}`)**:评估过——可读性优于四元组,3→4 正是元组开始伤人的临界点;但会 churn 所有既有 `load` 调用点的解构(`let (a,b,c)` → 字段访问),超出最小改动。**选四元组**因与现状 3 元组风格一致、diff 最小;可测性由 D6 的 seam 承接(不依赖 load 返回结构体)。
- **权衡**:改返回类型 touch 所有 `load` 调用点(生产 `tui/mod.rs:337` / `379` + `session/mod.rs` 测试解构 5 处 + write helper / tui 测试 write)。编译器驱动 arity 改全;但**编译器只管 arity、不管第 4 元素是否被用**——真正的接线守护见 D6。

### D4 serde 放置与 `StepStatus` 序列化形态
- `ActivePlan`/`ActiveStep`(tui::app)、`StepStatus`(tool::plan)加 `#[derive(Serialize, Deserialize)]`(**additive**,保留 `StepStatus` 的 `Copy`)。`session` 已依赖 `tui::app`(`TranscriptBlock` 即在此),依赖方向不变。
- `StepStatus` 用**默认 externally-tagged**(变体名 `"Pending"`/`"InProgress"`/`"Done"`)。**注意勿与 `update_plan` 的输入枚举 `ReportedStatus`(`in_progress`/`done`,snake_case、仅 2 变体)混淆**——那是工具**入参**契约(见 builtin-tools),`StepStatus` 是**内部/持久化**态,wire 格式不同、零碰撞。

### D5 resume 恢复 = 纯视觉恢复,复用既有清除/折叠
把 `load` 第 4 元素写入 `state.current_plan`。此后一切走既有逻辑:面板渲染不变;新一轮 user turn 在既有 choke point 清空;全 `Done` + `Ready` 时既有折叠规则生效。恢复路径**不碰 agent-loop / history**。
- **rationale**:「持久化」的本意是「resume 不丢进度展示」,不是「跨会话续跑任务」。视觉恢复与现有 `current_plan` 生命周期天然一致,零新增语义面。
- **两路径机制不同**:`--resume` 是**运行时 hot-swap**(picker 选中后事件循环内 `take_pending_session_switch`,有可变 `state`);`--continue` 是**启动期构造**(`prepare_session_startup` 返回 `SessionStartup`,此处无 `state`)。`SessionStartup` 现无 plan 字段、`run_tui` 构造 `AppState` 也不设 `current_plan`,故 `--continue` 若不改造会**静默丢弃** plan——两路都须接线,机制不同,统一到 D6 的 seam。

### D6 抽 plan-only seam `apply_loaded_plan`(接线可测性)
`load` 3→4 元组仅令调用点因 arity **编译错**、强制补绑第 4 元素;但**编译器不强制该元素被使用**——写成 `_plan` 即可编译且静默丢弃 plan,`clippy` 只会催绑 `_`、不报错。「plan 是否真塞进 `current_plan`」是本 change 的**唯一实质接线**,却落在编译器盲区。故抽**纯 sync 函数** `apply_loaded_plan(state: &mut AppState, plan: Option<ActivePlan>)`,函数体即 `state.current_plan = plan;`;`--resume`(picker)与 `--continue`(经 `SessionStartup.plan`)两路末尾都调它,并对它直接做**状态断言**(`Some`→还原 / `None`→不建空面板)——这是「还原是否发生」的自动化守护。
- **为何 seam 只管 `current_plan`(不统一整个 hot-swap)**:picker hot-swap 的其余副作用纯函数覆盖不了、两路机制也不同——`agent_history` 是 `Arc<AsyncMutex>`(`app.rs:460`)须 `.lock().await`;`input_tx.send(SetProvider)` 需 `input_tx`;`session_meta = meta` 改的是 `run_tui` 局部(**丢了它 autosave 会用旧 `id` 覆盖写回旧文件**);而 `--continue` 的 `history`/`transcript` 在启动期(`mod.rs:133`/`168`)已被 move 消耗,供不出「统一 hot-swap seam」的入参。两路唯一真正共享的就是 `current_plan = plan` 这一句。故 seam 收窄至此,其余副作用留各自调用点原地。
- **备选(不抽 seam、两路各自内联 `state.current_plan = plan`)**:弃——赋值落在 async 事件循环 / `run_tui` 函数体,不可单测,`_`-drop bug 无守护;抽出一句成纯函数即可 headless 断言。

## Risks / Trade-offs

- **前向不兼容(降级)**:旧二进制读含 `Plan` 行的新 session 命中现有「未知 tag → `Err`」→ 整会话加载失败。→ **Mitigation**:单机 CLI、无跨版本共享会话;不为此放宽 `load` 的严格 tag 校验(那会牺牲「损坏即报错」的现有保证)。在 proposal / CHANGELOG 点明「升级后写出的会话回退旧二进制不可读」。
- **`load` 返回类型变更**波及调用点与测试。→ **Mitigation**:编译器强制找全 arity;既有 round-trip 测试同步更新为四元组(本就该覆盖 plan)。
- **接线在编译器盲区**:`load` 4 元组只强制补绑、不强制使用,`_`-drop 会让 `current_plan` 永不还原而一切「绿」。→ **Mitigation**:D6 的 plan-only `apply_loaded_plan` seam + 对其的状态断言(tasks §3.4 / §3.5)。
- **恢复态与 `phase` 交互**:折叠依赖「全 Done + `Ready`」。`--continue` 冷加载新建 `AppState` 即 `Ready`;`--resume` picker 仅在启动时打开(`open_session_picker` 唯一调用于 `mod.rs:174`、`mode==Resume`),用户选中触发 hot-swap 时尚无 turn 跑过、phase 仍 `Ready`(无会话内重开 picker 的路径)。若 plan 全 `Done` 则折叠、否则完整——均为既有渲染派生,无需新代码。以既有 insta 快照锁定两态。
- **plan 与对话内容不同步**:`current_plan` 配对的是 `transcript`(二者一起落盘、一起还原),**非** `history`;compact 只动 `history` 不动 `transcript`,故面板与其上方的 `submit_plan` / `update_plan` 卡片视觉配对始终一致。resume 仅恢复面板视觉、不据 plan 驱动执行,不产生不一致。

## Migration Plan

无数据迁移:新字段 additive。旧 session 无 `Plan` 行 → `None`,行为等价升级前。回退 = 撤销本 change 后,新写出的含 `Plan` 行会话对旧代码不可读(见 Risks 前向不兼容),但对话数据(Meta/Msg/Block)结构未变。

## Open Questions

无。resume 语义(视觉恢复 vs 执行续接)已在 proposal 拍定为视觉恢复,执行续接明确 out-of-scope。
