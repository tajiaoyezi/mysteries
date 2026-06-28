## MODIFIED Requirements

### Requirement: TOML 配置解析

系统 SHALL 将 TOML 文本解析为 `RawConfig`,其字段一律为 `Option`,**缺失字段 → `None`**(用以表达「未设置」供分层 merge 继承)。`provider` 为嵌套表(`id` / `kind` / `base_url` / `auth_type`):其中 `id` 为**逻辑 provider 名**(如 `openai` / `anthropic` / `deepseek` / 自定义名),作凭据键与逻辑身份,与 `kind`(wire 协议族)**正交并存**;`id` MUST 为可选(`#[serde(default)]`),旧 `config.toml` 无该字段时 MUST 解析为 `None` 且**照常成功**(不破既有读取)。`Config` / `RawConfig` MUST NOT 含 `api_key` 字段 —— 凭据一律走 `CredentialChain`,不经配置。解析失败 MUST 返回 `ConfigError`,不得 panic。

#### Scenario: 部分字段的 TOML 解析为 Some / None

- **WHEN** 解析一段只设了 `model` 与 `[provider] kind` 的 TOML
- **THEN** 得到的 `RawConfig` 中 `model`、`provider.kind` 为 `Some`,未出现的 `max_iterations` / `timeout_secs` / `provider.base_url` / `provider.id` 为 `None`

#### Scenario: 旧 config 无 provider.id 照常解析

- **WHEN** 解析一段 `[provider]` 仅含 `kind` / `auth_type`(无 `id`)的旧 TOML
- **THEN** 解析成功,`provider.id` 为 `None`(向后兼容,不报错)

#### Scenario: 含 provider.id 的 TOML 解析

- **WHEN** 解析一段 `[provider]` 含 `id = "deepseek"`、`kind = "openai"` 的 TOML
- **THEN** `provider.id` 为 `Some("deepseek")`、`provider.kind` 为 `Some(OpenAi)`(二者并存)

#### Scenario: 非法 TOML

- **WHEN** 解析一段语法非法的 TOML
- **THEN** 返回 `ConfigError`(不 panic)

### Requirement: 解析为运行配置(默认与必填校验)

系统 SHALL 将 merge 后的 `RawConfig` resolve 为运行用 `Config`:`max_iterations` / `timeout_secs` 未设时套用文档化的默认常量;`model` 与 `provider.kind` 仍缺失时 MUST 返回 `ConfigError`(§9 配置非法 = 致命),不得以静默默认掩盖。`Config.provider` SHALL 含逻辑 `id`(`String`,resolve 后必有值):`provider.id` 设置时取所设值;**缺失(`None`)时 MUST 回落为 `kind` 的默认凭据名**(`OpenAi`→`"openai"`、`Anthropic`→`"anthropic"`、`Mock`→`"mock"`),使旧 config(无 `id`)的凭据解析行为与本 change 前一致(向后兼容)。

#### Scenario: 完整配置 resolve

- **WHEN** resolve 一个设了 `model` 与 `provider.kind`、但未设 `max_iterations` 的 `RawConfig`
- **THEN** 得到 `Config`,`model` / `provider.kind` 取所设值,`max_iterations` 取默认常量

#### Scenario: provider.id 缺失回落 kind 默认名(向后兼容)

- **WHEN** resolve 一个 `provider.kind = OpenAi` 但未设 `provider.id` 的 `RawConfig`
- **THEN** `Config.provider.id == "openai"`(回落 kind 默认凭据名);若 `kind = Anthropic` 则回落 `"anthropic"`

#### Scenario: provider.id 设置时取所设值

- **WHEN** resolve 一个 `provider.kind = OpenAi`、`provider.id = "deepseek"` 的 `RawConfig`
- **THEN** `Config.provider.id == "deepseek"`(逻辑 id 与 kind 并存、不被回落覆盖)

#### Scenario: 缺必填字段致命

- **WHEN** resolve 一个未设 `model`(或未设 `provider.kind`)的 `RawConfig`
- **THEN** 返回 `ConfigError`(指出缺失字段),不 panic、不静默默认

### Requirement: 配置写入(merge 持久化)

系统 SHALL 提供把部分字段 **merge** 写入 user `config.toml` 的能力(read-modify-write):读现有 `config.toml`(不存在则当空)→ 覆盖指定字段(如 `provider.id` / `provider.kind` / `provider.base_url` / `model`)→ **保留所有其他字段**后序列化回写。`provider.id`(逻辑 provider 名)MUST 随 patch 写入,使后续 `select_provider` 能据之注入凭据名。MUST NOT 整文件覆盖而丢失用户既有配置(如 `max_iterations` / `model_context_window` / `compact_trigger_ratio` 等)。路径由调用方注入以便临时文件测试;写入失败 SHALL 返回错误(不静默)。

#### Scenario: merge 写保留其他字段

- **WHEN** `config.toml` 含 `max_iterations = 40` 与 `model = "old"`,对 `model` 写入 `"new"`
- **THEN** 回写后 `model = "new"` 且 `max_iterations = 40` 仍在(其他字段未丢失)

#### Scenario: 写入逻辑 id 与 kind/base_url

- **WHEN** 以 patch(`provider_id = "deepseek"`、`provider_kind = OpenAi`、`base_url = Some("https://api.deepseek.com")`、`model = "deepseek-v4-pro"`)写入
- **THEN** 回写后 `[provider]` 含 `id = "deepseek"`、`kind = "openai"`、`base_url = "https://api.deepseek.com"`,`model = "deepseek-v4-pro"`,其他既有字段保留

#### Scenario: 文件不存在则新建

- **WHEN** user `config.toml` 不存在时写入 `model = "m"`
- **THEN** 新建该文件并含 `model = "m"`,不报「文件缺失」错误
