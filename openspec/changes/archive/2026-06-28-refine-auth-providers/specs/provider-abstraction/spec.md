## ADDED Requirements

### Requirement: Provider 凭据名构造注入

真实 provider 实现(`OpenAiProvider` / `AnthropicProvider`)SHALL 支持在**构造时注入用于解析 API key 的「凭据名」**,并在 `complete` 中用该名经 `CredentialChain` resolve 密钥,而非固定使用其 wire kind 的默认名。这使「逻辑 provider 身份」(凭据键)与「wire 协议族」(kind)解耦——例如一个 `kind=OpenAi` 的 provider 可用凭据名 `deepseek` 解析,与 `openai` 键分离。**未注入凭据名的既有构造路径**(`new` / `default` / 既有带 timeout 构造器)MUST 回落到 kind 默认名(`OpenAi`→`"openai"`、`Anthropic`→`"anthropic"`),使既有 provider 行为(含「凭据缺失 → `ProviderError::Auth`」)**逐字节不变**。凭据名注入 MUST NOT 改变 `Provider` trait 签名、MUST NOT 触网;凭据解析失败仍在 `complete` 内、HTTP 之前以 `ProviderError::Auth` fail-fast(凭据为辅助前置,非网络期)。

#### Scenario: 注入凭据名后按该名解析(离线)

- **WHEN** 以注入凭据名 `"deepseek"` 构造一个 `kind=OpenAi` 的 provider,其 `CredentialChain` 仅含 `"openai"` 键(不含 `"deepseek"`),调用 `complete`
- **THEN** 返回 `ProviderError::Auth`(按注入名 `"deepseek"` 解析未命中,**未**回落误用 `"openai"`),且解析在 HTTP 之前、不触网

#### Scenario: 未注入凭据名回落 kind 默认名(零回归)

- **WHEN** 以既有默认构造路径(不注入凭据名)构造 `OpenAiProvider`,其 `CredentialChain` 为空
- **THEN** 按 kind 默认名 `"openai"` 解析未命中 → `ProviderError::Auth`,与本 change 前行为一致(既有 provider 单测保持绿)

#### Scenario: 注入凭据名命中则不因 Auth 失败(离线)

- **WHEN** 以注入凭据名 `"deepseek"` 构造 provider,其 `CredentialChain` 含 `"deepseek"` 键
- **THEN** `complete` 的凭据前置解析命中(不返回 `ProviderError::Auth`),即按注入名而非 kind 名取密钥;构造期不触网
