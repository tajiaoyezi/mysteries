## MODIFIED Requirements

### Requirement: 会话快照落盘

系统 SHALL 提供 `SessionStore`,把一次会话落盘为 `<root>/sessions/<uuid>.jsonl`——行式 tagged record:首行 `Meta`(`SessionMeta`),其后 `agent_history` 逐条 `Msg`、`transcript` 逐块 `Block`,并可选一条 `Plan`(`ActivePlan`,即当前「执行中 / 完成的计划进度面板」状态),每行一个 `serde_json` 序列化的 `SessionLine`。`write` SHALL 接收当前计划参数 `Option<&ActivePlan>`:为 `Some` 时写入恰好一条 `Plan` 行,为 `None` 时不写 `Plan` 行。每次保存 SHALL **全量重写**整文件(使 compact 等历史缩短能被反映);因全量重写,文件内 SHALL 至多保留最近一次写入的那一条 `Plan` 行(旧 plan 不残留)。`sessions/` 目录不存在时 SHALL 先创建。

#### Scenario: 写入含工具调用的会话

- **WHEN** `write(meta, history=[System, User, Assistant{tool_calls}, ToolResult, Assistant], transcript=[User, Tool, Assistant], plan=None)`
- **THEN** 文件逐行含 1 个 `Meta` + 5 个 `Msg` + 3 个 `Block`、无 `Plan` 行,每行为合法 JSON

#### Scenario: 全量重写反映 compact

- **WHEN** 先以 10 条 history `write`,再以 compact 后的 4 条 history `write` 同一会话
- **THEN** 文件仅含 4 条 `Msg`(旧 6 条不残留)

#### Scenario: 写入携带 plan 进度

- **WHEN** `write(meta, history, transcript, plan=Some(ActivePlan{title, 2 步}))`
- **THEN** 文件含恰好一条 `Plan` 行,其 `ActivePlan` 内容与传入值逐字段相等

#### Scenario: 全量重写只留最新 plan

- **WHEN** 先以 `plan=Some(A)` `write`,再以 `plan=Some(B)` `write` 同一会话
- **THEN** 文件仅含一条 `Plan` 行、内容为 `B`(A 不残留)

### Requirement: 会话加载与还原

`SessionStore::load(id)` SHALL 逐行解析,按 tag 分派回 `(SessionMeta, Vec<Message>, Vec<TranscriptBlock>, Option<ActivePlan>)`,与写入前逐字段一致(round-trip)。`Plan` tag SHALL 归入返回的第四项 `Option<ActivePlan>`;文件**无 `Plan` 行时该项为 `None`**(向后兼容:早于本能力写出的会话照常加载);文件含**多于一条 `Plan` 行时 SHALL 返回 `Err`**(仿 `Meta` 重复,维持 store「异常即报错」一贯性;全量重写下正常至多一条)。分派 MUST NOT 依赖 `Meta` / `Msg` / `Block` / `Plan` 的行间顺序。任一行非法 JSON 或未知 tag → SHALL 返回 `Err`(不静默跳过)。文件 SHALL 恰含一个 `Meta` 行;零个或多于一个 `Meta` → SHALL 返回 `Err`。

#### Scenario: round-trip 完整还原

- **WHEN** `write`(含 `plan=Some(_)`)后 `load` 同一 id
- **THEN** 得到的 `meta` / `history` / `transcript` / `plan` 与写入值逐字段相等

#### Scenario: 行序无关分派

- **WHEN** 会话文件里 `Meta` / `Block` / `Msg` / `Plan` 行交错排列
- **THEN** `load` 仍把各行正确归入对应容器

#### Scenario: 无 Plan 行向后兼容

- **WHEN** `load` 一个仅含 `Meta` / `Msg` / `Block`、无 `Plan` 行的旧会话
- **THEN** 返回的第四项为 `None`,其余三项照常还原,不报错

#### Scenario: 多条 Plan 行报错

- **WHEN** 会话文件(手工构造)含两条 `Plan` 行 A、B
- **THEN** `load` 返回 `Err`(仿 `Meta` 重复)

#### Scenario: 损坏行报错

- **WHEN** 会话文件含一行非法 JSON 或未知 tag
- **THEN** `load` 返回 `Err`,不返回部分结果

#### Scenario: Meta 行缺失或重复报错

- **WHEN** 会话文件零个 `Meta` 行,或含多于一个 `Meta` 行
- **THEN** `load` 返回 `Err`

## ADDED Requirements

### Requirement: plan 进度类型序列化与 resume 恢复

`ActivePlan` / `ActiveStep`(`src/tui/app.rs`)与 `StepStatus`(`src/tool/plan.rs`)SHALL 可 `Serialize` / `Deserialize` 且 round-trip 保值;为 **additive derive**——既有 `Clone` / `Debug` / `PartialEq` / `Eq` 及 `StepStatus` 的 `Copy` 语义 MUST 不变(勿替换 derive 列)。`--resume`(`SessionPicker` 选中,**运行时 hot-swap**)与 `--continue`(**启动期经 `SessionStartup` 构造**,非 hot-swap)两路 SHALL 经**统一 plan-only seam `apply_loaded_plan(state, plan)`**(函数体即 `state.current_plan = plan`)把 `load` 回传的 `Option<ActivePlan>` 落为运行时 `current_plan`(`Some` → 面板状态就位;`None` → 无面板);MUST 复用既有 `load` 路径、不新增独立加载通道。两路其余会话还原副作用(`agent_history` / provider / `transcript` 等)**各自处理、不纳入该 seam**(它们涉 async / `input_tx` / run_tui 局部,且两路机制不同)。**因编译器仅强制 `load` 调用点补绑第 4 元素、不强制其被使用(`_`-drop 会静默丢弃 plan、clippy 不报错),该 seam SHALL 有直接状态断言守护**。还原后 plan 的**展示与生命周期语义**(清除时机、完成折叠、仅视觉恢复不执行续接)见 `tui-shell`。

#### Scenario: ActivePlan round-trip

- **WHEN** 序列化一个含 2 步、其一 `status=Done` 且带 `validation_result=Some(_)`、另一 `status=Pending` 的 `ActivePlan` 再反序列化
- **THEN** 得到与原值相等的 `ActivePlan`(含每步 `description` / `validation` / `status` / `validation_result`)

#### Scenario: resume 还原 current_plan

- **WHEN** 一个曾以 `plan=Some(ActivePlan)` 落盘的会话被 `--resume` 选中并 hot-swap,末尾经 `apply_loaded_plan`
- **THEN** 运行时 `current_plan` 被还原为该 `ActivePlan`

#### Scenario: resume 无 plan 的会话不建空面板

- **WHEN** 一个从未写过 `Plan` 行的会话经 `apply_loaded_plan` 还原(`plan=None`)
- **THEN** 运行时 `current_plan` 为 `None`,不误建空面板

#### Scenario: list_sessions 忽略 Plan 行

- **WHEN** `list_sessions` 枚举一个含 `Plan` 行的会话(`--resume` picker 的数据源)
- **THEN** 该会话摘要正常产出(`first_user` 取首个 `User` 消息、不被 `Plan` 行污染),不报错

#### Scenario: --continue 还原 current_plan

- **WHEN** 一个曾以 `plan=Some(_)` 落盘的会话经 `--continue` 启动(`prepare_session_startup` 的 Continue 分支)
- **THEN** 返回的 `SessionStartup.plan` 为 `Some(_)`,经 `apply_loaded_plan` 后运行时 `current_plan` 被还原
