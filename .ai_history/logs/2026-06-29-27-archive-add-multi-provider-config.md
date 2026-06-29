# 2026-06-29 · 27 · archive add-multi-provider-config

## 决策

- **config 从「单 active provider」升级为「多 provider 映射 + active 选择器」** | 主导:用户(`/models` 要切到任意已 auth provider,含自定义,而非当前 provider 的 kind/base_url 此前不被持久化)| 这是 `/models` epic 的**地基(3 之 1)**
- **D1 schema = map-of-tables `[providers.<id>]` + 顶层 `active`** | 用户拍板 | 弃 array-of-tables(id 重复在字段、upsert 要遍历查重)
- **D2 运行时 `Config` 不变,`resolve` 收敛单 active** | providers 列表不进运行时 `Config`(Change 2 再按需暴露给 `/models`)| 下游 `select_provider`/装配/`run_tui` 零改动
- **D3 向后兼容 = `RawConfig` 同容新旧字段 + resolve 优先级 + write 总是迁移** | `providers` 非空→新 schema、否则旧回落;`write_config` 把旧单 provider 迁入 map、只输出新 schema(写一次即迁移,不长期维护双形态)
- **D4 merge:providers 键并集 + 同 id 字段级;`active` project 覆盖 user**
- **D5 resolve active 四情形** | 命中→该 profile;未设单家→那家;未设多家→`MissingField("active")`;未知 id→`InvalidValue`;选定 profile 缺 `kind`/`model`→`MissingField`
- **D6 `write_config` upsert** | 迁旧(其 id 不在 map 才迁)→ `providers[patch.id]={kind,base_url,model}` → set `active` → 清旧 `provider`/`model` → 保留其他 → `BTreeMap` 有序序列化;`ConfigWritePatch` 接口不变、**`auth login` 生产代码零改动**(仅 write 落盘语义变)
- **审查(卡点 A,同 25/26 先例)**:`unimplemented!()`/最小桩出真红;发现并修 `missing_model` **假绿**(其 fixture 加顶层 `model="legacy"`,使旧路径红灯回 `MissingField("provider.kind")` ≠ 期望 → 真红→绿,附带验证 providers 优先级压过旧顶层 model);§5.2 **连带**改 `cli.rs` auth 测试断言读 `raw.active` + `raw.providers[id]`

## 变更

- `src/config/mod.rs`:+`RawProviderProfile`;`RawConfig` +`active`/+`providers`(保留旧 `provider`/`model`);+`merge_providers`/`merge_provider_profile`;`resolve` 拆 `resolve_multi_provider` + `resolve_legacy_provider`;`write_config` 改 upsert + `migrate_legacy_into_providers`;+§2–§4 测试 + 既有 write 测试断言更新
- `src/cli.rs`:§5.2 —— `run_auth_login_*` / WPS 测试改读 `raw.active` + `raw.providers[id]`(write 落盘形态变更的连带;生产代码不变)
- 运行时 `Config` / `select_provider` / `auth login` 生产代码**零改动**
- 验证:`cargo test` 294 lib + 1 e2e / 2 ignored;`cargo clippy --all-targets -D warnings` 零警告;`openspec validate --strict` 过;向后兼容冒烟(旧 config resolve OK + `auth login` 迁移到新 schema)
- archive:`changes/add-multi-provider-config` → `changes/archive/2026-06-29-add-multi-provider-config`;`specs/config-layering` MODIFIED「解析为运行配置」+「配置写入」+ ADDED「多 provider 配置 schema 与 merge」

## 待决

- **Change 2(epic ②)**:provider 注册表(内置模型目录,四家)+ 运行时热切(`Agent`/`Compacting` 加 `set_provider` + `SetProvider` 消息 + `run_agent_task` 重建)
- **Change 3(epic ③)**:`/models` TUI 模态 picker(↑↓ 浏览 provider+模型、选中发 `SetProvider`)
- **紧随本提交的小改**:WPS CodingPlan auth **去掉 model-select** 步、写默认 `zhipu/glm-5.2`,模型经 `/model`(现成)/ `/models`(epic ③)切换 —— 让 WPS 与其它预设「只填 key、模型默认」对齐
- `resolve` 暴露全部 provider profiles 给 `/models`(Change 2 设计)

## 引用

- change:`add-multi-provider-config`(D1–D6 见 design.md;archive 路径 `changes/archive/2026-06-29-add-multi-provider-config`)
- 前置 change:`add-config-layering`(07,parse/merge/resolve/write_config 原型)、`add-wps-ai-provider`(26,WPS provider)、`add-first-run-onboarding`(25,卡点 A 先例)
- session 主导:用户提「`/models` 切 provider+模型(像 opencode)」→ 架构核对(运行时换 provider 不存在 / config 单 provider / credentials 无 kind·base_url)→ 拆 3-change epic → 本 change(地基)brainstorm(map+active / 运行时 Config 不变 / 向后兼容)→ propose → 子 agent implement(卡点 A:`missing_model` 假绿修正)→ 主 agent review(独立 test/clippy、核 D4/D5/D6、抽查 §5.2 连带)
