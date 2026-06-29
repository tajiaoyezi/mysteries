## MODIFIED Requirements

### Requirement: 解析为运行配置(默认与必填校验)

系统 SHALL 将 merge 后的 `RawConfig` resolve 为运行用 `Config`(仍为**单个 active** provider + model,运行时结构不变)。**当 `providers` map 非空时(新 schema)**,SHALL 按 `active` 选定一家 provider profile:`active` 命中 map 中某 id → 取该 profile;`active` 未设但 map **恰一家** → 取那家;`active` 未设且**多家** → 返回 `ConfigError::MissingField("active")`;`active` 指向 map 中不存在的 id → 返回 `ConfigError::InvalidValue`。**否则(无 `providers` map)回落旧单 provider**(`[provider]` + 顶层 `model`,等价于一家 active)。无论新旧路径:`max_iterations` / `timeout_secs` 未设套默认常量;选定 provider 的 `kind` 与其 `model` 仍缺失 MUST 返回 `ConfigError`(配置非法 = 致命,不静默默认);`Config.provider.id` 取 profile 键(新 schema)或旧 `provider.id`(缺失时回落 `kind` 默认凭据名:`OpenAi`→`"openai"`、`Anthropic`→`"anthropic"`、`Mock`→`"mock"`,向后兼容)。

#### Scenario: 完整(旧单 provider)配置 resolve

- **WHEN** resolve 一个设了 `model` 与 `provider.kind`、但未设 `max_iterations`、无 `providers` map 的 `RawConfig`
- **THEN** 得到 `Config`,`model` / `provider.kind` 取所设值,`max_iterations` 取默认常量

#### Scenario: provider.id 缺失回落 kind 默认名(向后兼容)

- **WHEN** resolve 一个 `provider.kind = OpenAi` 但未设 `provider.id`、无 `providers` map 的 `RawConfig`
- **THEN** `Config.provider.id == "openai"`(回落 kind 默认凭据名);若 `kind = Anthropic` 则回落 `"anthropic"`

#### Scenario: provider.id 设置时取所设值

- **WHEN** resolve 一个 `provider.kind = OpenAi`、`provider.id = "deepseek"`、无 `providers` map 的 `RawConfig`
- **THEN** `Config.provider.id == "deepseek"`(逻辑 id 与 kind 并存)

#### Scenario: 新 schema 按 active 选定 provider

- **WHEN** resolve 一个 `providers` 含 `wps`(kind=OpenAi, base_url=W, model="m-wps")与 `deepseek`、`active = "wps"` 的 `RawConfig`
- **THEN** `Config.provider.id == "wps"`、`provider.kind == OpenAi`、`provider.base_url == Some(W)`、`Config.model == "m-wps"`

#### Scenario: active 未设但仅一家则取那家

- **WHEN** `providers` 仅含 `deepseek`、未设 `active`
- **THEN** resolve 取 `deepseek` 那家(无需显式 active)

#### Scenario: active 指向不存在的 id 报错

- **WHEN** `providers` 含 `wps`,但 `active = "nope"`
- **THEN** 返回 `ConfigError::InvalidValue`(不静默选别家)

#### Scenario: active 未设且多家报错

- **WHEN** `providers` 含 `wps` 与 `deepseek` 两家、未设 `active`
- **THEN** 返回 `ConfigError::MissingField("active")`(需显式指定,不自行假设)

#### Scenario: 缺必填字段致命

- **WHEN** resolve 一个无 `providers` map 且未设 `model`(或未设 `provider.kind`)的 `RawConfig`
- **THEN** 返回 `ConfigError`(指出缺失字段),不 panic、不静默默认

### Requirement: 配置写入(merge 持久化)

系统 SHALL 把 `ConfigWritePatch` **upsert** 进 user `config.toml` 的多 provider schema(read-modify-write):读现有 `config.toml`(不存在则当空)→ **若旧 `[provider]` + 顶层 `model` 存在且其逻辑 id 不在 `providers` map,先迁入 map**(`{kind, base_url, model}`)→ 写 `providers[patch.provider_id] = { kind: patch.provider_kind, base_url: patch.base_url, model: patch.model }` → 设 `active = patch.provider_id` → 清除旧 `[provider]` / 顶层 `model` 字段 → **保留所有其他 provider 条目与其他配置字段**(`max_iterations` / `model_context_window` 等)→ 序列化回写(仅新 schema)。MUST NOT 整文件覆盖而丢失其他 provider 或其他字段。路径由调用方注入以便临时文件测试;写入失败 SHALL 返回错误(不静默)。`ConfigWritePatch` 接口不变(`auth login` 调用方零改动)。

#### Scenario: upsert 保留其他 provider 并设 active

- **WHEN** `config.toml` 已含 `[providers.deepseek]`,以 patch(`provider_id="wps"`、`provider_kind=OpenAi`、`base_url=Some(W)`、`model="m-wps"`)写入
- **THEN** 回写后 `[providers.deepseek]` 仍在、新增 `[providers.wps]`(kind=openai / base_url=W / model=m-wps),`active = "wps"`

#### Scenario: 迁移旧单 provider 后再 upsert

- **WHEN** `config.toml` 为旧 schema(`model = "m-ds"` + `[provider] id="deepseek" kind="openai" base_url="D"`),以 patch(`provider_id="wps"`, …)写入
- **THEN** 回写后旧 deepseek 迁为 `[providers.deepseek]`(model=m-ds/base_url=D)、新增 `[providers.wps]`、`active="wps"`,顶层 `model` 与 `[provider]` 旧字段不再存在

#### Scenario: 写入保留其他配置字段

- **WHEN** `config.toml` 含 `max_iterations = 40`,对其 upsert 一个 provider
- **THEN** 回写后 `max_iterations = 40` 仍在(非 provider 字段未丢失)

#### Scenario: 文件不存在则新建

- **WHEN** user `config.toml` 不存在时 upsert 一个 provider patch
- **THEN** 新建该文件,含 `[providers.<id>]` 与 `active`,不报「文件缺失」错误

## ADDED Requirements

### Requirement: 多 provider 配置 schema 与 merge

系统 SHALL 支持多 provider 持久化 schema:顶层 `active: Option<String>` + `providers: Option<Map<id, profile>>`,profile = `{ kind, base_url: Option, model, auth_type: Option }`(map 键 = 逻辑 provider id)。`parse` SHALL 读取该 schema(serde),且旧单 provider config(无 `providers` / `active`)MUST 照常解析为二者 `None`(向后兼容,不报错)。`merge`(user / project)SHALL:`providers` map **键取并集**、**同 id 的 profile 按字段级 merge**(project 覆盖)、不同 id 并存;`active = project.active.or(user.active)`。序列化 SHALL 用有序 map(`BTreeMap`),使写出确定性。

#### Scenario: parse 新 schema

- **WHEN** 解析含 `active = "wps"` 与 `[providers.wps]`(kind/base_url/model)、`[providers.deepseek]` 的 TOML
- **THEN** `RawConfig.active == Some("wps")`,`providers` 含 `wps` 与 `deepseek` 两条 profile

#### Scenario: 旧 config 无 providers 照常 parse(向后兼容)

- **WHEN** 解析一段旧 schema(`model` + `[provider]`,无 `providers` / `active`)的 TOML
- **THEN** 解析成功,`active` 与 `providers` 均为 `None`(不报错)

#### Scenario: merge providers 键并集 + 同 id 字段级覆盖

- **WHEN** user `providers` 含 `a` 与 `b`(`b.model="u"`),project `providers` 含 `b`(`b.model="p"`)与 `c`,对二者 merge
- **THEN** 合并后 `providers` 含 `a`、`b`(`model="p"`,project 覆盖)、`c`(三家并存)

#### Scenario: merge active project 覆盖

- **WHEN** user `active = "a"`、project `active = "c"`
- **THEN** 合并后 `active = Some("c")`(project 覆盖)
