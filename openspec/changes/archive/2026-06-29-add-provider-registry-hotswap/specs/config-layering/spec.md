## ADDED Requirements

### Requirement: 全部 provider profiles 解析

在既有「解析为运行配置(收敛单 active)」之外,系统 SHALL 提供从合并后的 `RawConfig` 解析出**全部**已配 provider profiles 的能力,供运行时切换与 `/models` 浏览。每条 profile 暴露 `id` / `kind` / `base_url` / `model` / `auth_type`。该能力为运行时 `Config` 解析的旁路,**不**改变 `resolve` 收敛单 active 的既有行为。

#### Scenario: 新 schema 暴露全部 providers

- **WHEN** `RawConfig` 含 `[providers.anthropic]` 与 `[providers.wps]`(无论 `active` 指向谁)
- **THEN** 返回两条 profile,各带其 `id` / `kind` / `base_url` / `model`

#### Scenario: 旧单 provider 回落为单条

- **WHEN** `RawConfig` 无 `providers` map,仅旧 `[provider]` + 顶层 `model`
- **THEN** 返回单条 profile(等价于那一家 active)

#### Scenario: 无任何 provider 返回空

- **WHEN** `RawConfig` 既无 `providers` 也无旧 `[provider]`
- **THEN** 返回空列表

#### Scenario: 不完整 profile 被跳过

- **WHEN** 某 `[providers.<id>]` 缺 `kind` 或 `model`(不可切换)
- **THEN** 该条被跳过(不进结果),其余可用 profile 正常返回(一家不完整不致整体失败)
