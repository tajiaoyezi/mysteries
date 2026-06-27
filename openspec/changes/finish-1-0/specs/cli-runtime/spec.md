## ADDED Requirements

### Requirement: 配置超时注入 provider

`select_provider` SHALL 用 `config.timeout_secs` 构造所选 provider 的 per-attempt 超时(经 provider 的 timeout-taking 构造器注入 `RetryPolicy` 的 `attempt_timeout = Duration::from_secs(config.timeout_secs)`);重试次数沿用默认常量。OpenAi 与 Anthropic arm 均 MUST 注入该超时(替代此前硬编码 30s)。注入 MUST NOT 触发网络(仍构造期)。

#### Scenario: provider 用配置超时构造

- **WHEN** `config.timeout_secs = 12`,以 OpenAi(或 Anthropic)调用 `select_provider`
- **THEN** 返回的 provider 其 per-attempt 超时为 12s(经其 `RetryPolicy.attempt_timeout` 断言),构造期不触网
