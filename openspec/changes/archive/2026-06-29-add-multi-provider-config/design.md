## Context

现 config 只持久化**单个** active provider(`[provider]` + 顶层 `model`):`write_config`(`auth login` 调用)每次**整体覆盖** provider 段,非 active provider 的 kind/base_url 不被保存(`credentials` 只存 key)。`/models` 要切到任意已 auth 的 provider(含自定义),必须先持久化每家完整配置。本 change 升级 config 为「多 provider 映射 + active 选择器」并保持向后兼容,是 `/models` epic 的地基。

约束:运行时 `Config` 尽量不变(下游 `select_provider`/装配零改动);config 纯逻辑、TDD;向后兼容旧 `config.toml`。

## Goals / Non-Goals

**Goals:**
- config.toml 支持 `active` + `[providers.<id>]{kind,base_url,model}`;parse / merge / resolve / write 全链路打通。
- `write_config` upsert(保留其他 provider + 迁移旧单 provider + 设 active)。
- 旧单 provider 配置照常 resolve(向后兼容)。

**Non-Goals:**
- **不**改运行时 `Config` 结构(仍单 active provider + model)。
- **不**做 provider 注册表 / 内置模型目录 / 运行时热切(Change 2)。
- **不**做 `/models` TUI(Change 3)。
- **不**改 `select_provider` / 装配 / `auth login` 交互流程(只换 `write_config` 的落盘语义,patch 接口不变)。

## Decisions

- **D1 schema = map-of-tables + 顶层 `active`**(用户拍板)。`[providers.<id>]{kind,base_url,model}`,id 即表键(天然唯一、upsert 直接按键写)。**备选**:array-of-tables(弃:id 重复在字段里、upsert 要遍历查重)。

- **D2 运行时 `Config` 不变,`resolve` 收敛到单 active。** providers 列表**不**进运行时 `Config`(本 change 用不到;Change 2 再按需暴露 resolved providers 给 `/models`)。**理由**:下游 `select_provider`/装配/`run_tui` 零改动,爆炸半径最小。**备选**:`Config` 直接带 providers 列表(弃:本 change 无消费方,YAGNI)。

- **D3 向后兼容 = `RawConfig` 同容新旧字段 + resolve 优先级 + write 迁移。** `RawConfig` 加 `active`/`providers`(全 `Option`),保留旧 `provider`/`model`。`resolve` 优先级:`providers` 非空 → 新 schema(按 active);否则回落旧 `provider`+`model`。`write_config` **总是迁移**:把旧 `provider`+`model`(若存在且未在 map 中)迁进 map,只输出新 schema。**理由**:旧文件只读照常 work,写一次即迁移,不长期维护双形态。

- **D4 merge:providers map 键并集、同 id 字段级覆盖;`active` project 覆盖 user。** 同 id 的 profile 按字段 merge(与既有 provider 嵌套 merge 同语义),不同 id 并存;`active = project.active.or(user.active)`。**理由**:与现有「字段级 merge」一致;project 层定义 provider 是少见但合法的覆盖。

- **D5 `resolve` 的 active 选择规则:**
  - `active` 设且命中 → 该 profile;
  - `active` 设但 map 无此 id → `ConfigError::InvalidValue`("active references unknown provider");
  - `active` 未设但**恰一家** → 用那家;
  - `active` 未设且**多家** → `ConfigError::MissingField`("active",需显式指定);
  - 无任何 provider(新 map 空且无旧 `provider`)→ `MissingField`("model"/"provider.kind",沿用既有致命语义)。

- **D6 `write_config` upsert + 迁移(read-modify-write):** 读现有 raw → 若旧 `provider`+`model` 存在且其 id 不在 map,先迁入 map(`{kind,base_url,model}`)→ upsert `providers[patch.id] = {patch.kind, patch.base_url, patch.model}` → 设 `active = patch.id` → 清空旧 `provider`/`model` 字段 → 保留 `max_iterations` 等其他字段 → 序列化(仅新 schema)。`patch`(`ConfigWritePatch`)接口不变,`auth login` 调用方零改动。

## Risks / Trade-offs

- **merge / resolve 复杂度上升** → 充分 TDD:新 schema resolve、旧回落、active 四种情形(命中/未知/单家/多家)、map 合并、write 迁移 + 保留,逐一钉死。
- **`BTreeMap<String, _>` 序列化** → 键有序,写出确定性(利于测试断言 + diff 稳定);选 `BTreeMap` 而非 `HashMap`。
- **迁移仅在首次 `write_config` 发生** → 用户若只读不写旧格式则不迁移(可接受;resolve 仍兼容读)。
- **运行时 Config 不带 providers 列表** → Change 2 做 `/models` 时需新增一个「resolve 出全部 provider profiles」的能力(届时设计),本 change 不预埋。
