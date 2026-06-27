# cli-runtime Specification

## Purpose
TBD - created by archiving change add-cli-assembly. Update Purpose after archive.
## Requirements
### Requirement: 配置驱动的 provider 选择

系统 SHALL 提供 `select_provider(&Config, CredentialChain) -> Result<Box<dyn Provider>, AssemblyError>`,按 `config.provider.kind` 选择:`OpenAi` → 真实 `OpenAiProvider`(`base_url` 取 `config.provider.base_url`,有则用、无则默认 endpoint;凭据移交 `CredentialChain`);`Anthropic` → 真实 `AnthropicProvider`(`base_url` 取 `config.provider.base_url`,有则用、无则默认 endpoint;凭据移交 `CredentialChain`);`Mock` → `MockProvider`(固定 canned 脚本)。选择 / 构造过程 MUST NOT 发起网络请求(凭据缺失等在 run 时经 `ProviderError::Auth` 暴露,非选择期)。

#### Scenario: OpenAi 选中真实 provider(离线构造)

- **WHEN** `config.provider.kind = OpenAi`,调用 `select_provider`
- **THEN** 返回 `Ok(Box<dyn Provider>)`(真实 `OpenAiProvider`),构造期不触网

#### Scenario: Anthropic 选中真实 provider(离线构造)

- **WHEN** `config.provider.kind = Anthropic`,调用 `select_provider`
- **THEN** 返回 `Ok(Box<dyn Provider>)`(真实 `AnthropicProvider`),构造期不触网

#### Scenario: Mock 可离线跑

- **WHEN** `config.provider.kind = Mock`,调用 `select_provider`
- **THEN** 返回 `Ok` 的 `MockProvider`(固定 canned 脚本),无需网络 / 凭据即可被调用

### Requirement: 两层配置加载(缺失容忍)

系统 SHALL 提供 `load_config(user_path, project_path) -> Result<Config, _>`:对每个路径,存在则读取并 `config::parse`,**不存在则容忍**(当作空层 `RawConfig::default`);再 `config::merge(user, project)` 后 `config::resolve` 为 `Config`。路径由调用方注入(home / 默认路径解析在 main 薄胶水,不在本函数),以便用临时目录离线单测。文件存在但 TOML 非法、或 resolve 缺必填字段 MUST 返回错误(不静默)。

#### Scenario: 两层均存在,project 覆盖 user

- **WHEN** user 与 project 两份 TOML 均存在(project 覆盖部分字段),调用 `load_config`
- **THEN** 得到 merge + resolve 后的 `Config`,project 的字段覆盖 user、未设字段继承

#### Scenario: 缺失文件被容忍

- **WHEN** user 路径不存在、project 路径存在且完整
- **THEN** 不报「文件缺失」错误,以 project 单层(+ 默认)resolve 出 `Config`

#### Scenario: 存在但非法

- **WHEN** 某存在的配置文件 TOML 非法(或 resolve 缺 `model` / `provider.kind`)
- **THEN** 返回错误(不 panic、不静默默认)

### Requirement: stdin y/n 权限 decider

系统 SHALL 提供 `StdinDecider`(impl `PermissionDecider`),用于 CLI(TUI 前过渡):对 `RequiresConfirmation` 工具,展示工具名 + 参数后读取一行,经一个**可单测的纯解析函数**判定 —— `y` / `yes`(忽略大小写与首尾空白)→ `Allow`,其余(含空行 / EOF)→ `Deny`(fail-safe)。stdin 读取 MUST 为薄壳(异步下经 `spawn_blocking`),决策解析与读取解耦。

#### Scenario: 确认输入 y 放行

- **WHEN** 解析用户输入 `"y"`(或 `"Y"` / `"yes"` / 带首尾空白)
- **THEN** 解析为 `PermissionDecision::Allow`

#### Scenario: 非确认输入拒绝(fail-safe)

- **WHEN** 解析用户输入为 `"n"` / 空行 / 其他任意串 / EOF
- **THEN** 解析为 `PermissionDecision::Deny`

### Requirement: 端到端装配与运行

系统 SHALL 提供 `assemble_agent(provider, &Config, decider) -> Agent`:注册全部 **7 个内置工具**、以 `config.model` 与 `config.max_iterations` 构造 `Agent`。CLI 入口 SHALL 在装配后 seed `[System, User(prompt)]` 历史,并经 `StdoutSink` 流式驱动 `Agent.run`。本能力 MUST 可由 `MockProvider` + 临时目录 + 脚本化 decider **端到端离线**验证(hermetic)。

#### Scenario: 装配出的 Agent 持 7 工具并按 config 构造

- **WHEN** 以一个 `Config`(`model` / `max_iterations`)+ 注入的 provider + decider 调用 `assemble_agent`
- **THEN** 得到的 `Agent` 的工具 registry 含 7 个内置工具,且其 `model` / `max_iterations` 取自 `config`

#### Scenario: 端到端多轮任务(hermetic)

- **WHEN** 注入 `MockProvider` 脚本(轮1 → `write_file` 的 tool_call、轮2 → 终复文本),cwd = 临时目录,decider 脚本化放行,跑 `Agent.run`
- **THEN** 临时目录中对应文件被创建 / 改动,最终返回终复文本,工具结果与历史正确(全程离线、不触网)

### Requirement: 配置超时注入 provider

`select_provider` SHALL 用 `config.timeout_secs` 构造所选 provider 的 per-attempt 超时(经 provider 的 timeout-taking 构造器注入 `RetryPolicy` 的 `attempt_timeout = Duration::from_secs(config.timeout_secs)`);重试次数沿用默认常量。OpenAi 与 Anthropic arm 均 MUST 注入该超时(替代此前硬编码 30s)。注入 MUST NOT 触发网络(仍构造期)。

#### Scenario: provider 用配置超时构造

- **WHEN** `config.timeout_secs = 12`,以 OpenAi(或 Anthropic)调用 `select_provider`
- **THEN** 返回的 provider 其 per-attempt 超时为 12s(经其 `RetryPolicy.attempt_timeout` 断言),构造期不触网

