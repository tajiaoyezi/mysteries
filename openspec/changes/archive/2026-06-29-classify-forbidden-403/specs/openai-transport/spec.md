## MODIFIED Requirements

### Requirement: 超时与指数退避重试

`OpenAiProvider` MUST 以 per-attempt `tokio::time::timeout` 包裹每次请求尝试;超时 → `ProviderError::Timeout`。对 `429` / `5xx` / 网络错误 / `Timeout` MUST 以指数退避重试至上限;对 `Auth` 及其他致命错误(非可重试 4xx、`Decode`)MUST 立即失败不重试。重试耗尽 MUST 返回最后一次错误(对 Agent loop 即致命)。「何种 HTTP 结果 → 何 `ProviderError` 变体 / 是否可重试」MUST 由一个**与 reqwest 解耦的纯分类逻辑**决定(吃 status `u16` / 抽象传输错误 kind),以便离线单测。

#### Scenario: 限流 / 服务端错误触发重试

- **WHEN** 注入的尝试连续返回可重试结果(如 `429` → `RateLimited`)再返回成功
- **THEN** 经指数退避重试后返回成功的 `ModelResponse`,尝试次数 = 失败次数 + 1

#### Scenario: 401 鉴权失败不重试

- **WHEN** 注入的尝试返回 `401`(分类为 `Auth`)
- **THEN** 立即返回 `Err(ProviderError::Auth)`,只尝试一次、不重试

#### Scenario: 403 forbidden 不重试且非 Auth

- **WHEN** 注入的尝试返回 `403`
- **THEN** 立即返回 fatal `ProviderError::Transport`,message 含 `forbidden (403)` 及模型/配额提示;**不**映射为 `Auth`;只尝试一次、不重试

#### Scenario: 重试耗尽返回最后错误

- **WHEN** 注入的尝试在重试上限内始终返回可重试错误
- **THEN** 达上限后返回最后一次的 `ProviderError`(`RateLimited` / `Timeout` / 可重试 `Transport`)

#### Scenario: 单次尝试超时记为 Timeout

- **WHEN** 一次尝试耗时超过 per-attempt 超时预算(测试用虚拟时间推进)
- **THEN** 该尝试被记为 `ProviderError::Timeout` 并触发重试
