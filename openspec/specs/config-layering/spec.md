# config-layering Specification

## Purpose
TBD - created by archiving change add-config-layering. Update Purpose after archive.
## Requirements
### Requirement: TOML 配置解析

系统 SHALL 将 TOML 文本解析为 `RawConfig`,其字段一律为 `Option`,**缺失字段 → `None`**(用以表达「未设置」供分层 merge 继承)。`provider` 为嵌套表(`kind` / `base_url` / `auth_type`)。`Config` / `RawConfig` MUST NOT 含 `api_key` 字段 —— 凭据一律走 `CredentialChain`,不经配置。解析失败 MUST 返回 `ConfigError`,不得 panic。

#### Scenario: 部分字段的 TOML 解析为 Some / None

- **WHEN** 解析一段只设了 `model` 与 `[provider] kind` 的 TOML
- **THEN** 得到的 `RawConfig` 中 `model`、`provider.kind` 为 `Some`,未出现的 `max_iterations` / `timeout_secs` / `provider.base_url` 为 `None`

#### Scenario: 非法 TOML

- **WHEN** 解析一段语法非法的 TOML
- **THEN** 返回 `ConfigError`(不 panic)

### Requirement: 字段级分层 merge(project 覆盖 user)

系统 SHALL 以**字段级**方式 merge user 层与 project 层的 `RawConfig`:project 的 `Some` 字段覆盖 user 对应字段,project 的 `None` 字段继承 user 的值;`provider` 嵌套表同样按字段级 merge(非整表替换)。两层皆 `None` 的字段,merge 结果仍为 `None`。

#### Scenario: project 覆盖标量字段、继承未设字段

- **WHEN** user 设 `model = "a"` 且 `timeout_secs = 30`,project 设 `model = "b"`(未设 `timeout_secs`),对二者 merge
- **THEN** merge 结果 `model = Some("b")`(project 覆盖)、`timeout_secs = Some(30)`(继承 user)

#### Scenario: provider 嵌套字段级 merge

- **WHEN** user 设 `provider.kind = "openai"` 且 `provider.base_url = "u"`,project 仅设 `provider.base_url = "p"`,对二者 merge
- **THEN** merge 结果 `provider.base_url = Some("p")`(project 覆盖)、`provider.kind = Some(OpenAi)`(继承 user)

#### Scenario: 两层皆未设

- **WHEN** user 与 project 均未设某字段
- **THEN** merge 结果该字段为 `None`

### Requirement: 解析为运行配置(默认与必填校验)

系统 SHALL 将 merge 后的 `RawConfig` resolve 为运行用 `Config`:`max_iterations` / `timeout_secs` 未设时套用文档化的默认常量;`model` 与 `provider.kind` 仍缺失时 MUST 返回 `ConfigError`(§9 配置非法 = 致命),不得以静默默认掩盖。

#### Scenario: 完整配置 resolve

- **WHEN** resolve 一个设了 `model` 与 `provider.kind`、但未设 `max_iterations` 的 `RawConfig`
- **THEN** 得到 `Config`,`model` / `provider.kind` 取所设值,`max_iterations` 取默认常量

#### Scenario: 缺必填字段致命

- **WHEN** resolve 一个未设 `model`(或未设 `provider.kind`)的 `RawConfig`
- **THEN** 返回 `ConfigError`(指出缺失字段),不 panic、不静默默认

