# provider-registry Specification

## Purpose
TBD - created by archiving change add-provider-registry-hotswap. Update Purpose after archive.
## Requirements
### Requirement: 内置 provider 模型目录

系统 SHALL 提供一份内置模型目录,以 **provider-id**(与 auth preset / config `[providers.<id>]` 的 id 对齐)为键,枚举每家 provider 的可用模型,作为运行时切换与 `/models` 浏览的数据源。目录为实现常量,随网关 / 官方模型变更通过改常量 + 单测维护。

#### Scenario: 已知 provider-id 返回其模型列表

- **WHEN** 以 `"anthropic"` / `"openai"` / `"deepseek"` / `"wps"` 查询目录
- **THEN** 返回该 id 对应的非空模型列表(如 `"wps"` 含 `zhipu/glm-5.2` 等)

#### Scenario: 未知 provider-id 返回 None

- **WHEN** 以目录未收录的 id(如自定义 `"my-llm"`)查询
- **THEN** 返回 `None`(区分「无目录」与「目录为空」),调用方据此对 custom provider 只列其已配 model

#### Scenario: WPS 与 deepseek 不被归并到 openai 目录

- **WHEN** 查询 `"wps"` 或 `"deepseek"`(二者 `kind` 均为 `OpenAi`)
- **THEN** 返回各自模型集(WPS 的 `zhipu/glm-*` 等、deepseek 的 `deepseek-v4-*`),不返回 OpenAI 官方模型集——目录按 id 而非 `ProviderKind` 区分

