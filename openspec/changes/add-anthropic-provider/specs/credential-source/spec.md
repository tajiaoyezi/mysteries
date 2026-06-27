## MODIFIED Requirements

### Requirement: 环境变量凭据来源 EnvCredentialSource

`EnvCredentialSource` SHALL 将 provider 名映射到约定的环境变量(含 `"openai"` → `OPENAI_API_KEY`、`"anthropic"` → `ANTHROPIC_API_KEY`),命中变量则以其值构造 `SecretString` 返回,未设置 → `None`。其对环境的读取 MUST 可注入替换,以便单测离线、确定性,不依赖进程级真实环境状态。

#### Scenario: openai 映射命中环境变量

- **WHEN** 环境(或注入的等价 lookup)中 `OPENAI_API_KEY` 为某非空值,调用 `resolve("openai")`
- **THEN** 返回 `Some(SecretString)`,其 `expose_secret()` 等于该值

#### Scenario: anthropic 映射命中环境变量

- **WHEN** 环境(或注入的等价 lookup)中 `ANTHROPIC_API_KEY` 为某非空值,调用 `resolve("anthropic")`
- **THEN** 返回 `Some(SecretString)`,其 `expose_secret()` 等于该值

#### Scenario: 环境变量未设置

- **WHEN** 环境(或注入的 lookup)中不含目标变量,调用 `resolve("openai")`
- **THEN** 返回 `None`
