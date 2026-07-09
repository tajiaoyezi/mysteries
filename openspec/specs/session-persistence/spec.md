# session-persistence Specification

## Purpose
TBD - created by archiving change add-session-persistence. Update Purpose after archive.
## Requirements
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

### Requirement: 最近会话查找

`SessionStore::latest()` SHALL 返回 `sessions/` 下修改时间最新的 `.jsonl` 对应的会话 id;空目录或无 `.jsonl` → `None`;非 `.jsonl` 文件 SHALL 忽略。

#### Scenario: 取 mtime 最新

- **WHEN** `sessions/` 有多个 `.jsonl`、mtime 不同
- **THEN** `latest` 返回 mtime 最大者的 id

#### Scenario: 空目录

- **WHEN** `sessions/` 下无 `.jsonl` 文件
- **THEN** `latest` 返回 `None`

#### Scenario: 忽略非 jsonl 文件

- **WHEN** `sessions/` 下混有非 `.jsonl` 文件与 `.jsonl` 会话
- **THEN** `latest` 只在 `.jsonl` 中取最新,忽略其他文件

### Requirement: uuid 会话标识

新会话 id SHALL 由 `uuid` v4 生成;会话文件名 SHALL 为 `<id>.jsonl`;`SessionMeta.id` SHALL 与文件名 id 一致。

#### Scenario: 新会话 id 合法且唯一

- **WHEN** 连续生成两个 session id
- **THEN** 两者均为合法 uuid 且互不相等

#### Scenario: meta.id 与文件名一致

- **WHEN** 新建会话并 `write`
- **THEN** 文件名为 `<id>.jsonl`,`Meta` 行的 `id` 与文件名 id 相同

### Requirement: --resume 恢复会话

CLI SHALL 支持 `--resume`(无参):启动进 TUI 后弹**会话选择 modal**(`SessionPicker`,见 `tui-shell`),列出历史会话(短 id / 时间 / 首条 `User` 摘要,mtime 逆序);用户选中一个 → **运行时 hot-swap** 该会话——`SessionStore::load(id)` 得 `(meta, history, transcript)`,`history` 首条 `System` 消息 SHALL 替换为当前 `DEFAULT_SYSTEM_PROMPT`(仅还原对话内容),替换运行中的 `agent_history` 与 `transcript`,并以 `meta.provider` / `meta.model` 经 **profile 查找**(取 `ProviderProfile` kind / base_url,非仅 id)还原 provider;`meta.provider` 不在当前配置或凭据缺失时 SHALL 回退 startup 默认 provider 并以 `Notice` 提示、不 panic。hot-swap 后续落盘 SHALL **续写选中会话文件**(不新建)。用户 `Esc` 取消 → 保持进入时的(新)会话。`sessions/` 无历史会话时 SHALL NOT 弹 picker,按新会话继续。无 `--resume` 时对话流程与现状一致,并为本次运行创建新会话。

#### Scenario: resume 弹列表并 hot-swap 还原

- **WHEN** 存在多个历史会话,以 `--resume` 启动,在 `SessionPicker` 中选中一个含工具调用的会话
- **THEN** `agent_history` 与 `transcript` hot-swap 为该会话内容,provider / model 取自其 `meta`,后续落盘续写该会话文件

#### Scenario: meta.provider 缺失回退

- **WHEN** 选中会话的 `meta.provider` 不在当前配置中(或其凭据缺失)
- **THEN** 回退 startup 默认 provider、给出 `Notice`,不 panic

#### Scenario: System 消息用当前默认

- **WHEN** hot-swap 的会话 `history[0]` 是建会话当时的旧 `System` 消息
- **THEN** 还原后 `history[0]` 为当前 `DEFAULT_SYSTEM_PROMPT`,其后对话内容不变

#### Scenario: Esc 取消保持新会话

- **WHEN** `--resume` 弹出 `SessionPicker` 后按 `Esc`
- **THEN** picker 关闭,保持进入时的新会话,不 hot-swap

#### Scenario: 无历史会话不弹

- **WHEN** `sessions/` 下无任何会话文件,以 `--resume` 启动
- **THEN** 不弹 picker,按新会话继续

### Requirement: 落盘容错不阻断对话

每轮保存失败(IO 错误)SHALL NOT 中断对话:核心 agent 循环继续,失败以 `Notice` 呈现,`agent_history` 不受影响。

#### Scenario: 写失败仅提示

- **WHEN** 某轮保存时 `SessionStore::write` 返回 IO `Err`
- **THEN** 该对话轮正常完成,`transcript` 追加一条 `Notice`,`agent_history` 不受影响

### Requirement: UI 类型序列化保真

`TranscriptBlock` / `ToolCard` / `ToolCardStatus` / `StatusSnapshot` SHALL 可 `Serialize` / `Deserialize` 且 round-trip 保值;既有 `Clone` / `Debug` / `PartialEq` 语义 MUST 不变。

#### Scenario: ToolCard round-trip

- **WHEN** 序列化一个含 `exit = Some(_)`、`truncated = true`、`status = Error` 的 `ToolCard` 再反序列化
- **THEN** 得到与原值相等的 `ToolCard`

### Requirement: --continue 续最近会话

CLI SHALL 支持 `--continue`(无参):启动时(进 TUI 前)加载最近会话续跑——`load(latest)` 得历史,首条 `System` 替换当前 `DEFAULT_SYSTEM_PROMPT`,以其 `meta.provider` / `meta.model` 还原 provider(缺失回退 + `Notice`),**不弹 picker**。`--resume` 与 `--continue` 同时给出时 SHALL 以 `--resume` 优先(弹 picker)。`sessions/` 无历史会话时按新会话继续。

#### Scenario: continue 续最近

- **WHEN** 存在历史会话,以 `--continue` 启动
- **THEN** 直接加载最近会话、还原历史与 provider,不弹 picker

#### Scenario: resume 优先于 continue

- **WHEN** 同时传 `--resume` 与 `--continue`
- **THEN** 走 `--resume`(弹 picker)

#### Scenario: continue 的 System 用当前默认

- **WHEN** `--continue` 加载的最近会话 `history[0]` 是旧 `System` 消息
- **THEN** 还原后 `history[0]` 为当前 `DEFAULT_SYSTEM_PROMPT`,其后对话不变

#### Scenario: continue provider 缺失回退

- **WHEN** `--continue` 的最近会话 `meta.provider` 不在当前配置中(或凭据缺失)
- **THEN** 回退 startup 默认 provider、给出 `Notice`,不 panic

### Requirement: 会话列表枚举

`SessionStore::list_sessions()` SHALL 返回 `sessions/` 下所有 `.jsonl` 的摘要 `Vec<SessionSummary { id, created_at, first_user }>`,按 mtime **逆序**(最新在顶);`first_user` = 该会话首个 `Msg(User(_))` 内容截断(≤ 60 字符),无 `User` 消息则 `None`。损坏文件(非法 JSON / 缺 `Meta`)SHALL **跳过、不整体失败**(列表容错,区别于 `load` 的严格 `Err`);非 `.jsonl` 文件 SHALL 忽略。

#### Scenario: mtime 逆序 + 首 User 摘要

- **WHEN** `sessions/` 有多个 `.jsonl`、mtime 不同
- **THEN** `list_sessions` 按 mtime 逆序返回,每项 `first_user` 为该会话首个 `User` 消息截断

#### Scenario: 损坏文件跳过

- **WHEN** `sessions/` 含一个损坏 `.jsonl` 与若干正常会话
- **THEN** `list_sessions` 跳过损坏项、返回其余正常会话,不整体报错

#### Scenario: 无 User 消息

- **WHEN** 某会话仅含 `Meta` 与 `System`、无 `User` 消息
- **THEN** 其 `first_user` 为 `None`

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

