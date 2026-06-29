## ADDED Requirements

### Requirement: `UserInput::SetProvider` 变体与 agent-task 热替

`UserInput` SHALL 新增 `SetProvider { id: String, model: String }` 变体,对称于既有 `SetModel`。`run_agent_task` SHALL 持有「全部已配 provider profiles」(启动时由 `resolve_provider_profiles` 解析)与重建凭据的能力,并新增处理 arm:收到 `SetProvider{id, model}` 后,按 `id` 取 profile、组瞬时运行配置(继承启动配置的 timeout / 压缩旋钮)、重建 `CredentialChain` 经 `select_provider` 造新 `Arc<dyn Provider>`,热替进 `agent`(及手动 `compacting`)并同步 model。切换不打断既有会话 history。

#### Scenario: 收到 SetProvider 后下一轮用新 provider

- **WHEN** agent-task 收 `SetProvider{ id, model }`(id 在 profiles 中、凭据齐备),随后收 `Prompt`
- **THEN** 该轮模型请求经新 provider 发出、用新 model;会话 history 跨切换保留

#### Scenario: 未知 id 发 Notice 不崩

- **WHEN** 收 `SetProvider{ id }` 而 `id` 不在 profiles 中
- **THEN** 上送 `AgentEvent::Notice`(提示未知 provider),保持当前 provider,task 不退出

#### Scenario: 缺凭据切换发 Notice 不崩

- **WHEN** 目标 provider 缺 API key,`select_provider` 报错
- **THEN** 上送 `AgentEvent::Notice`(提示凭据缺失),保持当前 provider,task 不退出
