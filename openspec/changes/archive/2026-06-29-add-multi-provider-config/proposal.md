## Why

为支撑后续 `/models`(运行时浏览 / 热切「已 auth 的所有 provider」及其模型,**含自定义 provider**),config 需从「单 active provider」升级为「多 provider 映射 + active 选择器」,并**持久化每家完整配置**(kind / base_url / model)。这是 `/models` epic 的**地基(3 个 change 的第 1 个)**:无多 provider 持久化,就无法切到非当前 provider(其 kind/base_url 此前不被保存)。

## What Changes

- **config.toml 新增多 provider schema**:顶层 `active = "<id>"` + `[providers.<id>] { kind, base_url, model }`(map-of-tables,id 即表键)。
- **parse**:读新 schema;**向后兼容**旧单 provider(`model` + `[provider]`)照常解析。
- **merge**:`providers` map 按 id 字段级合并(project 覆盖同 id、键取并集);`active` 由 project 覆盖 user。
- **resolve**:有 `providers` map → 按 `active` 解析那家为运行 `Config`(无 `active` 但仅一家 → 用那家;`active` 指向不存在的 id → `ConfigError`;无任何 provider → `MissingField`);否则**回落**旧单 provider(`[provider]` + `model`,等价于一家 active)。
- **write_config 改 upsert**:写入 `[providers.<id>]` + 设 `active=<id>`,**保留其他 provider**,并把旧单 provider(若存在)**迁移**进 map。`auth login` 因此从「覆盖唯一 provider」变为「追加 / 更新一家并设为 active」。
- **运行时 `Config` 结构不变**:仍解析为**单个 active** provider + model,下游 `select_provider` / 装配 **零改动**;providers 列表仅为持久化 + 后续 `/models`。

## Capabilities

### New Capabilities

(无)

### Modified Capabilities

- `config-layering`:
  - **MODIFIED**「解析为运行配置(默认与必填校验)」—— 增 `active` 选择 + 旧单 provider 回落。
  - **MODIFIED**「配置写入(merge 持久化)」—— 改 upsert 进 `[providers.<id>]` map + 设 `active` + 迁移旧单 provider + 保留其他。
  - **ADDED**「多 provider 配置 schema 与 merge」—— `active` + `[providers.<id>]` 的解析、map 合并、向后兼容契约。

## Impact

- **代码**:`src/config/mod.rs` —— `RawConfig` 加 `active: Option<String>` + `providers: Option<BTreeMap<String, RawProviderProfile>>`(新 `RawProviderProfile{ kind, base_url, model, auth_type? }`),保留旧 `provider`/`model` 字段;`merge` 加 map 合并;`resolve` 加 active 选择 + 旧回落;`write_config` 改 upsert + 迁移。**运行时 `Config` 不变**。无 UI 改动。
- **不做(后续 change)**:provider 注册表 + 运行时热切引擎(Change 2)、`/models` TUI 模态(Change 3)。
- **测试**:config 纯逻辑、IO 无关,走 TDD(fixture 字符串 + 临时目录)。
- **向后兼容**:旧 `config.toml` 照常 resolve;首次 `write_config` 把其迁移到新 schema。
