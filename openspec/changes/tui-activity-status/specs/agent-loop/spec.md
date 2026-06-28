## MODIFIED Requirements

### Requirement: 结构化观测事件(observer 变体)

系统 SHALL 提供 `AgentObserver`(`Send + Sync`,方法 `on_status` / `on_tool_call_started` / `on_tool_call_finished` / `on_usage`,**全部 default no-op**)与 `AgentStatus`(`Idle` / `CallingModel` / `ExecutingTool(String)` / `WaitingForPermission`),以及 `Agent::run_observed(history, ctx, sink, observer)`:在循环关键点经 observer 发结构化事件 —— 模型调用前 `StatusChanged(CallingModel)`;工具分发时 `StatusChanged(ExecutingTool(name))` 与 `on_tool_call_started{id, name, args, readonly}`(`readonly` 取自工具 `permission_level`);`RequiresConfirmation` 工具询问前 `WaitingForPermission`;工具产出结果后 `on_tool_call_finished{id, outcome}`(执行结果 / 拒绝 / 未知工具均以 `ToolOutcome` 上报);循环自然终止前 `Idle`。每次 `provider.complete` 返回后,若 `ModelResponse.usage` 为 `Some`,MUST 经 `observer.on_usage(&usage)` 上送该轮真实 token 用量;`usage` 为 `None` 的轮 MUST NOT 上送。`on_usage` 取 `&Usage`(`provider-abstraction` 已定义),**default no-op**。

既有 `Agent::run` 的契约(history 累积、终止条件、错误分流,见本能力既有 requirement)MUST 保持不变,且 `run` MUST 委托 `run_observed` 并传入 no-op observer —— `run` 的行为与本 change 前**逐字节一致**(`on_usage` default no-op 故不影响 `run`)。`AgentObserver` 方法的 default no-op MUST 使任何不关心观测的调用方零负担。

#### Scenario: 观测一轮工具调用的事件顺序

- **WHEN** 以 Mock 脚本「轮1 → 一个工具的 tool_call、轮2 → 终复文本」调用 `run_observed`,传入一个记录事件的 observer
- **THEN** observer 依次收到 `CallingModel` → `ToolCallStarted` →(`ExecutingTool` / 必要时 `WaitingForPermission`)→ `ToolCallFinished` → 下一轮 `CallingModel` → `Idle`,且 history 与 `run` 等价(`on_usage` 是否触发取决于脚本是否带 usage,不改变上述 status / tool 事件的相对顺序)

#### Scenario: run 委托后行为与原一致(零回归)

- **WHEN** 调用既有 `Agent::run`(不带 observer)跑任意脚本
- **THEN** 其 history、返回值、终止 / 错误行为与本 change 前完全一致(`run` 委托 `run_observed` + no-op observer,既有 agent-loop 测试保持绿)

#### Scenario: 权限拒绝仍上报工具完成

- **WHEN** 某 `RequiresConfirmation` 工具被 decider 拒绝
- **THEN** observer 收到 `WaitingForPermission` 后,该工具以 is_error 的 `ToolOutcome`(user denied)触发 `on_tool_call_finished`,且既有「denial 入 history、循环继续」行为不变

#### Scenario: 每轮 model 调用后上送 token 用量

- **WHEN** 以 Mock 脚本(其 `ModelResponse` 带 `usage: Some(Usage{..})`)调用 `run_observed`,传入记录事件的 observer
- **THEN** 该次 model 调用返回后 observer 收到 `on_usage` 携该轮 `Usage`;若某轮 `ModelResponse.usage` 为 `None` 则该轮不收到 `on_usage`;`run`(no-op observer)行为与本 change 前逐字节一致
