# agent-loop Delta

## ADDED Requirements

### Requirement: Plan 模式编排(mode-aware schema + 系统指令 + 纵深拒)

`Agent` SHALL 经 **setter** `set_permission_mode(Arc<Mutex<PermissionMode>>)`(仿 `set_strategy`,**不改 `Agent::new` 签名**;字段默认 `Arc::new(Mutex::new(Normal))`,故既有 `Agent::new` 调用与行为逐字节不变)接入一个**运行时可变的 `PermissionMode` 共享源**(克隆自 TUI 侧共享状态;headless 默认 `Normal`)。

每轮循环 **顶部读取一次 mode 快照**(`let mode = *lock` 单次、随即释锁),该轮的 schema 装配、指令注入、**及本轮 tool_call 循环里的每一次纵深拒 MUST 复用这同一个快照**,MUST NOT 在处理每个 tool_call 时重读 mutex:
- **mode-aware schema**:`ModelRequest.tools` 用 `registry.schemas_for(mode)` 取代 `registry.schemas()` —— `Plan` 期只下发只读 + plan_only 工具(schema-omit,见 tool-system)。
- **plan 系统指令**:`mode==Plan` 时 MUST 把一条 plan 模式 system 指令注入**该轮的 transient 请求 messages**(即 `strategy.prepare` 产出的 `Vec<Message>`、`ModelRequest` 之前;**MUST NOT** 入持久 `history`,否则逐轮累积并被存进 session 快照);语义三分支:**用户只是问 → 直接答;撞歧义/岔路 → 用 `ask_user` 弹选项让用户定;要执行任务 → 用 `submit_plan` 交带每步 `validation` 的结构化 plan**;只读、不改文件/不执行命令。非 Plan 不注入。
- **纵深拒(用快照,双向)**:① `mode==Plan` 且某 tool_call 的工具**非 `ReadOnly`**(schema-omit 之外的越界)→ MUST 直接产出 is_error 的 `ToolResult`、**不执行、不弹权限 UI**;② `mode!=Plan` 且某 tool_call 的工具 `plan_only`(如凭记忆硬发 `submit_plan`)→ 亦 MUST is_error 拒(对称防御)。二者 schema-omit 为主控制、纵深拒为辅,循环续跑。
- **快照封住中途翻转**:同一批 `[submit_plan, edit_file]` 中,`submit_plan` 批准会**在本轮 tool 循环中途**把共享 mode 翻 `Plan→AcceptEdits`;因纵深拒复用**轮顶快照(仍 Plan)**,`edit_file` 兄弟仍被拒;翻转只影响**下一轮**的新快照(届时全工具可用、执行已批准 plan)。

既有 `run` / `run_observed` 在非 Plan(默认 `Normal`)下行为 MUST 与本 change 前**逐字节一致**(`schemas_for(Normal)` 对未注册 plan_only 工具的 registry 与 `schemas()` 保序逐字节相等)。mode 源、注入、纵深拒逻辑 headless 可测(setter 注固定 mode 源 + Mock provider)。

#### Scenario: Plan 模式只下发只读 + plan_only

- **WHEN** mode 源置 `Plan`,registry 含只读 / 变更 / plan_only 工具,跑一轮(Mock provider)
- **THEN** 该轮 `ModelRequest.tools` 仅含只读 + plan_only 项(变更类被摘)

#### Scenario: Plan 模式注入三分支系统指令

- **WHEN** mode==`Plan` 跑一轮
- **THEN** 该轮 messages 含一条 plan 模式 system 指令(问答 / ask_user / submit_plan 三分支);mode==`Normal` 时该指令不出现

#### Scenario: Plan 期越界变更工具被纵深拒

- **WHEN** mode==`Plan`,模型发出一个 `Edit` / `Execute` 工具的 tool_call
- **THEN** 产出 is_error 的 `ToolResult`(plan 拒变更)入 history、工具不执行、不发权限 UI,循环续跑

#### Scenario: 同批 submit_plan + 变更工具,快照封住中途翻转

- **WHEN** mode==`Plan`,模型在**一条回复**里发 `[submit_plan, edit_file]` 两个 tool_call;submit_plan 批准在本轮 tool 循环中途把共享 mode 翻 `AcceptEdits`
- **THEN** `edit_file` 兄弟仍按**轮顶快照(Plan)**被纵深拒(不执行、未静默放行);翻转仅令**下一轮**新快照为 `AcceptEdits`

#### Scenario: 非 Plan 模式硬发 plan_only 工具被拒

- **WHEN** mode!=`Plan`(如 `Normal`),模型硬发一个 `plan_only` 工具(如 `submit_plan`)的 tool_call
- **THEN** 产出 is_error 的 `ToolResult`(对称防御)、工具不执行

#### Scenario: 非 Plan 零回归

- **WHEN** mode==`Normal`(默认)跑任意既有脚本
- **THEN** history / 终止 / 错误 / 事件与本 change 前一致(`schemas_for(Normal)` 等价既有;无 plan_only 工具时逐字节一致)
