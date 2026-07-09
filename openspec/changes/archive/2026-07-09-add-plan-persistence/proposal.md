## Why

Plan 模式已完整落地(`add-plan-mode` + `add-plan-progress` 均已 archive),但**执行进度面板的状态(`current_plan`)是纯 TUI 内存态、不进 session 快照**:`src/session/` 对 plan 零持久化。因此 `--resume` 一个曾经批准过 plan 的会话时,常驻的「执行中 / 完成的计划」面板**丢失**——虽然 transcript 里的 `submit_plan` / `update_plan` 卡片能恢复(它们是已持久化的 `TranscriptBlock`),但那块承载「哪步做完了、验收结果是什么」的进度面板无法重建。这是 `add-plan-mode` proposal 亲口划到 enrichment 的「plan 持久化」遗留项。

## What Changes

- **`SessionLine` 增加 `Plan(ActivePlan)` 变体**:session jsonl 除既有 `Meta` / `Msg` / `Block` 外,可承载一条 plan 进度快照。
- **`SessionStore::write` 接收当前 plan**:签名加 `Option<&ActivePlan>` 参数,`Some` 时写入一条 `Plan` 行;沿用既有「全量重写」模型,故文件里天然只保留最新一条 plan 快照。
- **`SessionStore::load` 回传 plan**:返回值增加 `Option<ActivePlan>`;文件含多条 `Plan` 行时返回 `Err`(仿 `Meta` 重复,维持 store「异常即报错」一贯性)。
- **`ActivePlan` / `ActiveStep`(`src/tui/app.rs`)与 `StepStatus`(`src/tool/plan.rs`)派生 `serde`**:使其可序列化进 session(additive derive,保留既有 `Copy` 等)。
- **抽纯 sync seam `apply_loaded_plan`(只设 `current_plan`)统一还原**:`--resume`(运行时 hot-swap 末尾)与 `--continue`(启动期构造后)两路都调它把 `load` 回传的 plan 落进 `state.current_plan`;两路其余会话还原副作用各自原地处理。既有 PlanProgress 面板渲染逻辑照原样把它画出来。
- **恢复语义 = 视觉恢复,非执行续接**:恢复后仅**展示**上次会话结束时的计划 / 进度面板(完成态自然折叠、中断态显完整),用户发出新一轮 prompt 后按 `tui-shell` 现有「新 turn choke point 清空 `current_plan`」规则清除。**明确 out-of-scope**:resume 后让 agent 接着跑未完成步骤(见 Impact 的排除项)。

## Capabilities

### New Capabilities
<!-- 无新增能力域;本 change 扩展两个既有能力的 requirement -->

### Modified Capabilities
- `session-persistence`:`SessionLine` 新增 `Plan` 变体;`write` / `load` 契约扩展以携带 plan 进度快照;新增 round-trip、向后兼容(旧 session 无 `Plan` 行 → `None`)、全量重写只留最新、`list_sessions` 忽略 `Plan` 行、`--continue` 还原等 requirement 与 scenario。
- `tui-shell`:新增 resume 时把持久化的 plan 塞回 `current_plan` 的恢复语义 requirement(**视觉恢复、非执行续接**;完成态折叠 / 中断态完整由既有面板逻辑派生)。

## Impact

- **Affected code**:
  - `src/session/mod.rs`:`SessionLine::Plan` 变体、`write` / `load` 签名与逻辑、**`read_session_summary` 的 `match` 补 `Plan(_)` 忽略臂**(否则 E0004 非穷尽)、round-trip 与兼容测试(headless 内核,强制 TDD)。
  - `src/tui/app.rs`:`ActivePlan` / `ActiveStep` 派生 `Serialize` / `Deserialize`(additive)。
  - `src/tool/plan.rs`:`StepStatus` 派生 `Serialize` / `Deserialize`(additive,**保留 `Copy`**)。
  - `src/tui/mod.rs`:autosave(`write_session_snapshot`)传入 `state.current_plan.as_ref()`;抽纯 sync seam `apply_loaded_plan`(只设 `current_plan`);`--resume`(picker hot-swap 末尾调用)与 `--continue`(`SessionStartup` 加 `plan` 字段 + 启动期构造后调用)两路共用该 seam,其余 hot-swap 副作用留各自原地。
- **UI / 设计规范**:本 change **不改任何视觉 / 组件契约**——PlanProgress 面板的渲染逻辑与外观**逐字节不变**,仅恢复喂给它的状态。该面板晚于 `设计规范/` 冻结日引入,`设计规范/` 未收录它;其视觉契约由 `openspec/specs/tui-shell`「执行中的计划进度面板(PlanProgress)」requirement + 既有 insta 快照承载(config.yaml 权威次序:code + tests > 设计规范)。故 **无 port / adapt / drop 偏差适用**。
- **依赖**:无新增依赖(`serde` 现成)。
- **向后兼容**:新版读旧 session(无 `Plan` 行)→ `current_plan = None`、面板不渲染,安全。**降级不兼容**:含 `Plan` 行的新 session 被旧版二进制(如已分发的 v1.1.0)读取会命中现有「未知 tag → `Err`」而整会话加载失败——即**升级后写出的会话,回退到旧二进制不可读**。考虑到单机 CLI、无跨版本共享会话的场景,此代价可接受(不为此放宽 `load` 的严格 tag 校验);CHANGELOG 明说。
- **明确不在本 change**:resume 后**执行续接**(agent 继续跑未完成步骤)。理由:plan 系统指令是 transient、明确 MUST NOT 入 `history`,续接需把 plan 上下文重建进 `history`,与现有 agent-loop 设计冲突,属独立特性。
