# config-layering Specification

## Purpose
TBD - created by archiving change add-config-layering. Update Purpose after archive.
## Requirements
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

### Requirement: 上下文压缩配置

运行配置 SHALL 含上下文压缩三项,均可经两层 TOML 分层 merge 覆盖(project 覆盖 user):

- `model_context_window: Option<u32>`(tokens)—— **未配 = 压缩禁用**(装配 `Passthrough`,行为同现状);
- `compact_trigger_ratio: f32` —— 默认 `0.8`,MUST 落在 `(0.0, 1.0]`,越界 SHALL 报配置错;
- `keep_recent_turns: u32` —— 默认 `1`(压缩时保留的最近完整轮数)。

`model_context_window` 配置后,装配层 SHALL 据之注入 `Compacting` 策略(否则保持 `Passthrough`)。三项的默认与既有配置项(如 `max_iterations`)一致地由 `resolve` 套用、可被配置覆盖。

#### Scenario: 默认值

- **WHEN** 配置未设压缩三项,`resolve` 得运行配置
- **THEN** `compact_trigger_ratio == 0.8`、`keep_recent_turns == 1`、`model_context_window == None`

#### Scenario: 分层 merge 覆盖

- **WHEN** user 配 `model_context_window = 128000`、project 覆盖 `compact_trigger_ratio = 0.7`
- **THEN** 合并后 `model_context_window == Some(128000)`、`compact_trigger_ratio == 0.7`、`keep_recent_turns` 取默认 `1`

#### Scenario: ratio 越界报错

- **WHEN** 配置 `compact_trigger_ratio = 1.5`(或 `0`)
- **THEN** `resolve` 返回配置错,不静默接受

#### Scenario: window 未配则压缩禁用

- **WHEN** `model_context_window` 未配
- **THEN** 装配层选用 `Passthrough`(压缩禁用),Agent 行为与无压缩时一致

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

