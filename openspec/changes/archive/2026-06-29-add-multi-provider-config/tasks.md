## 1. 类型(纯增量)

- [x] 1.1 `config/mod.rs`:定义 `RawProviderProfile { kind: Option<ProviderKind>, base_url: Option<String>, model: Option<String>, auth_type: Option<AuthType> }`(serde,字段全 `Option` + `#[serde(default)]`);`RawConfig` 加 `active: Option<String>` + `providers: Option<BTreeMap<String, RawProviderProfile>>`(`#[serde(default)]`),**保留**旧 `provider` / `model` 字段(向后兼容)
- 注:纯类型 / serde,正确性由 §2–§4 测试钉死

## 2. parse + merge 新 schema(TDD)

- [x] 2.1 【红】写测试:① parse 新 schema(`active="wps"` + `[providers.wps]` + `[providers.deepseek]`)→ `RawConfig.active=Some`、`providers` 两条;② parse 旧 schema(无 providers/active)→ 二者 `None`(向后兼容);③ merge providers 键并集 + 同 id 字段级覆盖;④ merge `active` project 覆盖 user。运行确认失败
- [x] 2.2 【绿】RawConfig 字段就位后 parse 经 serde 即过;实现 `merge` 的 providers map 合并(键并集、同 id 调既有 provider 字段级 merge)+ `active = project.active.or(user.active)`
- [x] 2.3 【重构】清理

## 3. resolve active 选择 + 旧回落(TDD —— 新行为,红灯停点)

- [x] 3.1 【红】写 `resolve` 测试覆盖 spec:① 新 schema `active` 命中 → 取那家 profile(`id`=键 / `kind` / `base_url` / `model`);② `active` 未设但单家 → 取那家;③ `active` 未设多家 → `MissingField("active")`;④ `active` 指向不存在 → `InvalidValue`;⑤ 无 `providers` → 旧单 provider 回落(既有行为不回归);⑥ 选定 provider 缺 `model`/`kind` → `MissingField`。先确认失败(非编译错;如需用最小桩使编译)→ 贴测试 + 红灯输出 **→ 停下等确认**
- [x] 3.2 【绿】`resolve` 加 providers 分支:`providers` 非空 → 按 `active` 选 profile(四情形,见 design D5)→ 组 `Config.provider` + `model`;否则走旧路径。复用既有默认套用 + ratio 校验 + id 回落
- [x] 3.3 【重构】清理

## 4. write_config upsert + 迁移(TDD)

- [x] 4.1 【红】写 `write_config` 测试:① 已有 `[providers.deepseek]` upsert `wps` → 两家都在 + `active="wps"`;② 旧单 provider(`model` + `[provider]`)upsert `wps` → 旧 deepseek 迁入 map + 新 wps + `active="wps"` + 旧 `[provider]`/顶层 `model` 字段消失;③ 保留 `max_iterations`;④ 文件不存在则新建。运行确认失败
- [x] 4.2 【绿】`write_config` 改 upsert:读 raw → 迁旧单 provider 入 map(若其 id 不在 map)→ `providers[patch.provider_id] = {kind, base_url, model}` → 设 `active` → 清旧 `provider`/`model` 字段 → 保留其他 → 序列化(仅新 schema)。`ConfigWritePatch` 接口不变
- [x] 4.3 【重构】清理

## 5. 全量校验(含连带测试更新)

- [x] 5.1 `cargo build` 通过、`cargo clippy --all-targets -D warnings` 零警告
- [x] 5.2 **更新连带测试**:既有断言「写入后 `raw.provider` / `[provider]`」的测试(`config/mod.rs` 的 write_config 测试、`cli.rs` 的 `run_auth_login_*` / WPS 测试)随落盘形态改为读 `raw.providers[id]` + `raw.active`(write_config 产物从 `[provider]` 变 `[providers.<id>]` 的连带;**生产代码 `auth login` 不变**,仅测试断言更新)
- [x] 5.3 `cargo test` 全绿(新增 §2–§4 + 既有 config/装配/auth 测试经 §5.2 更新后不回归)
- [x] 5.4 向后兼容冒烟:旧格式 `config.toml` resolve 成功;`cargo run -- auth login` 写一次后,文件变为新 schema(`active` + `[providers.*]`,旧 `[provider]`/`model` 消失)
- [x] 5.5 `openspec validate add-multi-provider-config --strict` 通过
