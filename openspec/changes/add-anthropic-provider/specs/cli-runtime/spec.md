## MODIFIED Requirements

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
