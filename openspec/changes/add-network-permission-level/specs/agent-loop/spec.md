## MODIFIED Requirements

### Requirement: 结构化观测事件(observer 变体)

系统 SHALL 提供 `AgentObserver`(`Send + Sync`,方法 `on_status` / `on_tool_call_started` / `on_tool_call_finished` / `on_usage`,**全部 default no-op**)与 `AgentStatus`(`Idle` / `CallingModel` / `ExecutingTool(String)` / `WaitingForPermission`),以及 `Agent::run_observed(history, ctx, sink, observer)`:在循环关键点经 observer 发结构化事件 —— 模型调用前 `StatusChanged(CallingModel)`;工具分发时 `StatusChanged(ExecutingTool(name))` 与 `on_tool_call_started{id, name, args, readonly}`,其中 `readonly` MUST 精确等价于 `permission_level == ReadOnly`(`Network` 为 false,不得标为“自动运行”);`Network` / `Edit` / `Execute` 工具询问前发 `WaitingForPermission`(命中 mode 自动放行时可无等待事件);工具产出结果后 `on_tool_call_finished{id, outcome}`(执行结果 / UserDenied / NetworkUnauthorizable / 未知工具均以 `ToolOutcome` 上报);循环自然终止前 `Idle`。每次 `provider.complete` 返回后,若 `ModelResponse.usage` 为 `Some`,MUST 经 `observer.on_usage(&usage)` 上送该轮真实 token 用量;`usage` 为 `None` 的轮 MUST NOT 上送。`on_usage` 取 `&Usage`(`provider-abstraction` 已定义),**default no-op**。

既有 `Agent::run` 的契约(history 累积、终止条件、错误分流,见本能力既有 requirement)MUST 保持不变,且 `run` MUST 委托 `run_observed` 并传入 no-op observer。对不含 Network 工具的既有脚本,`run` 行为与本 change 前逐字节一致；Network 工具仅增加权限决策,其执行后 history / outcome 结构不变。`AgentObserver` 方法的 default no-op MUST 使任何不关心观测的调用方零负担。

#### Scenario: 观测一轮工具调用的事件顺序

- **WHEN** 以 Mock 脚本「轮1 → 一个工具的 tool_call、轮2 → 终复文本」调用 `run_observed`,传入一个记录事件的 observer
- **THEN** observer 依次收到 `CallingModel` → `ToolCallStarted` →(`ExecutingTool` / 必要时 `WaitingForPermission`)→ `ToolCallFinished` → 下一轮 `CallingModel` → `Idle`,且 history 与 `run` 等价(`on_usage` 是否触发取决于脚本是否带 usage,不改变上述 status / tool 事件的相对顺序)

#### Scenario: run 委托后行为与原一致(零回归)

- **WHEN** 调用既有 `Agent::run`(不带 observer)跑一个不含 Network 工具的既有脚本
- **THEN** 其 history、返回值、终止 / 错误行为与本 change 前完全一致(`run` 委托 `run_observed` + no-op observer,既有 agent-loop 测试保持绿)

#### Scenario: Network observer 不误报 ReadOnly

- **WHEN** 模型发出一个 `PermissionLevel::Network` 的 tool_call
- **THEN** `on_tool_call_started.readonly == false`;需要询问时 observer 收到 `WaitingForPermission`,不得产生“只读 · 自动运行”语义

#### Scenario: 权限拒绝仍上报工具完成

- **WHEN** 某非 `ReadOnly`(`Network` / `Edit` / `Execute`)工具被 decider 拒绝
- **THEN** observer 收到 `WaitingForPermission` 后,该工具以 is_error 的 `ToolOutcome`(user denied)触发 `on_tool_call_finished`,且既有「denial 入 history、循环继续」行为不变

#### Scenario: 每轮 model 调用后上送 token 用量

- **WHEN** 以 Mock 脚本(其 `ModelResponse` 带 `usage: Some(Usage{..})`)调用 `run_observed`,传入记录事件的 observer
- **THEN** 该次 model 调用返回后 observer 收到 `on_usage` 携该轮 `Usage`;若某轮 `ModelResponse.usage` 为 `None` 则该轮不收到 `on_usage`;`run`(no-op observer)行为不受观测机制影响

### Requirement: Plan 模式编排(mode-aware schema + 系统指令 + 纵深拒)

`Agent` SHALL 经 setter `set_permission_mode(Arc<Mutex<PermissionMode>>)`接入一个运行时可变的 `PermissionMode` 共享源(克隆自 TUI 侧共享状态)；字段默认 `Arc::new(Mutex::new(Normal))`,headless 默认 Normal，既有 `Agent::new` 签名与调用行为不变。

每轮循环顶部 MUST 读取一次 mode 快照并随即释锁；该轮 schema 装配、指令注入及本轮 tool_call 循环里的每一次纵深拒 MUST 复用同一个快照,不得在处理每个 tool_call 时重读 mutex:

- **mode-aware schema**:`ModelRequest.tools` 经 `registry.schemas_for(mode)` 取得。Plan 期下发 `ReadOnly + Network + plan_only`,摘掉 `Edit / Execute`。
- **plan 系统指令**:`mode==Plan` 时 MUST 把一条 plan 模式指令注入该轮 transient 请求 messages(`strategy.prepare` 产出的 Vec、进入 ModelRequest 前),MUST NOT 入持久 history。指令语义为:用户只是问 → 直接答；撞歧义 / 岔路 → `ask_user`；要执行任务 → `submit_plan` 提交结构化 plan，且每一步 MUST 带可独立验收的 `validation`；本地研究只读,web 研究工具可用但每次会请求 Network 授权,不得编辑文件或执行命令。非 Plan 不注入。
- **纵深拒(双向)**:① `mode==Plan` 且工具为 `Edit` / `Execute` → 直接产出 is_error ToolResult,不执行、不弹权限 UI；`Network` MUST NOT 命中该拒绝,而是进入 gate / decider。② `mode!=Plan` 且工具 `plan_only` → is_error 拒,循环继续。
- **快照封住中途翻转**:同一批 `[submit_plan, edit_file]` 中,submit_plan 批准即使把共享 mode 翻为 `AcceptEdits`,edit_file 仍按轮顶 Plan 快照被拒；翻转只影响下一轮。

对不含 Network tool_call 的非 Plan 既有脚本,run / run_observed 行为 MUST 与本 change 前一致。Network 在 Normal / AcceptEdits 下新增询问、在 Yolo 下自动放行,属于刻意的安全行为变化。mode 源、schema、指令、纵深拒与 Network gate 均须 headless Mock 可测。

#### Scenario: Plan 模式只下发 ReadOnly + Network + plan_only

- **WHEN** mode 源置 `Plan`,registry 含 ReadOnly / Network / Edit / Execute / plan_only 工具,跑一轮(Mock provider)
- **THEN** 该轮 `ModelRequest.tools` 仅含 ReadOnly / Network / plan_only 项(Edit/Execute 被摘),顺序保持

#### Scenario: Plan 指令保留 validation 并加入联网授权语义

- **WHEN** mode==`Plan` 跑一轮
- **THEN** 从 Mock provider 实收的首条 System message 直接断言其含问答 / ask_user / submit_plan 三分支、每步可验收 `validation`、web research 每次需 Network 授权及禁止 Edit / Execute；expected 不得引用 `PLAN_MODE_INSTRUCTION` 常量自身；该指令不入 history，Normal 不注入

#### Scenario: Plan 期 Network Allow 后执行

- **WHEN** mode==`Plan`,模型发出一个提供 `authorizable=true` 专用 preview 的 Network tool_call,decider 返回 Allow
- **THEN** 该调用不被纵深拒,工具执行并把正常 ToolResult 入 history,循环续跑

#### Scenario: Plan 期 Network Deny 零网络并续跑

- **WHEN** mode==`Plan`,模型发出 Network tool_call,decider 返回 Deny
- **THEN** 工具不执行、WebFetcher 零调用、is_error ToolResult 入 history,循环续跑

#### Scenario: 未知 Network 工具在 Agent Loop 中 fail-closed

- **WHEN** mode 为 Normal / Yolo / Plan 任一,模型调用未 override preview 的 Network tool,decider 返回 Allow
- **THEN** gate 最终 `Deny(NetworkUnauthorizable(reason))`、工具不 execute、带 reason 的 is_error ToolResult 入 history供模型修正,循环进入下一轮；不得写成 user denied

#### Scenario: Plan 期越界变更工具被纵深拒

- **WHEN** mode==`Plan`,模型发出一个 `Edit` / `Execute` 工具的 tool_call
- **THEN** 产出 is_error ToolResult(plan 拒变更)入 history、工具不执行、不发权限 UI,循环续跑

#### Scenario: 同批 submit_plan + 变更工具,快照封住中途翻转

- **WHEN** mode==`Plan`,模型在一条回复里发 `[submit_plan, edit_file]`;submit_plan 批准在本轮中途把共享 mode 翻 `AcceptEdits`
- **THEN** edit_file 仍按轮顶 Plan 快照被纵深拒；翻转仅令下一轮新快照为 AcceptEdits

#### Scenario: 非 Plan 模式硬发 plan_only 工具被拒

- **WHEN** mode!=`Plan`(如 Normal),模型硬发一个 plan_only 工具(如 submit_plan)
- **THEN** 产出 is_error ToolResult、工具不执行

#### Scenario: 非 Plan 的旧三类工具零回归

- **WHEN** mode==Normal 跑一个不含 Network tool_call 的既有脚本
- **THEN** history / 终止 / 错误 / 事件与本 change 前一致；schema 仍含 ReadOnly / Edit / Execute 且顺序不变
