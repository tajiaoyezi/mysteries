## 1. 依赖 + 模块骨架

- [x] 1.1 `Cargo.toml` 加 `toml`(选版本并记由,§11);`cargo build` 通过
- [x] 1.2 建 `src/config/mod.rs`;`src/main.rs` 加 `mod config;`(仅声明,**不接线**);`cargo build` 通过(`config` dead_code 警告为预期,见 design D1)

## 2. 配置类型(serde)

- [x] 2.1 定义 `Config`(resolved:`provider: ProviderConfig` / `model: String` / `max_iterations: u32` / `timeout_secs: u64`)+ `RawConfig`(全 `Option` 字段,`#[serde(default)]`)+ `ProviderConfig` / `RawProviderConfig`(`kind` / `base_url: Option<String>` / `auth_type`,**无 `api_key`**)+ `ProviderKind { OpenAi, Anthropic, Mock }`(`rename_all = "lowercase"`)+ `AuthType { ApiKey }`(`rename_all = "snake_case"`,OAuth 注释占位)
- [x] 2.2 定义 `ConfigError`(`thiserror`:`Toml(String)` + `MissingField(...)`),置于 `config` 模块内(见 design D7)
- 注:纯类型 / 枚举,无逻辑,不走 red-green;正确性由 §3–§5 测试间接钉死

## 3. parse(TOML → RawConfig,TDD)

- [x] 3.1 【红】写 `parse` 测试:部分字段 TOML → 对应 `Some` / 未设 `None`(含 `provider` 嵌套);非法 TOML → `ConfigError`;确认失败
- [x] 3.2 【绿】实现 `parse(&str) -> Result<RawConfig, ConfigError>`(`toml` 反序列化,缺失 → `None`)
- [x] 3.3 【重构】清理

## 4. merge(字段级分层,TDD · §10 范围 4 核心)

- [x] 4.1 【红】写 `merge` 测试(fixture 字符串经 `parse`):project `Some` 覆盖标量、`None` 继承 user;`provider` 嵌套字段级(project 只改 `base_url`、继承 `kind`);两层皆未设 → `None`;确认失败
- [x] 4.2 【绿】实现 `merge(user, project) -> RawConfig`(字段级,project 覆盖;`provider` 嵌套递归 merge,见 design D3)
- [x] 4.3 【重构】清理

## 5. resolve(默认 + 必填校验,TDD)

- [x] 5.1 【红】写 `resolve` 测试:完整 raw → `Config`(未设 `max_iterations` 取默认常量);缺 `model` / 缺 `provider.kind` → `ConfigError::MissingField`(不 panic);确认失败
- [x] 5.2 【绿】实现 `resolve(RawConfig) -> Result<Config, ConfigError>`(`DEFAULT_MAX_ITERATIONS` / `DEFAULT_TIMEOUT_SECS` 兜底;`model` / `provider.kind` 缺失致命,见 design D4)
- [x] 5.3 【重构】清理

## 6. 收尾

- [x] 6.1 `cargo build` 通过、`cargo test` 全绿且**离线**(fixture 字符串,无真实 FS)、`cargo fmt`(可选 `cargo clippy`)
- [x] 6.2 自检:§7 / §10 范围 4 与 `config-layering` spec 的 3 条 ADDED requirements 全部有测试落点;**§10 范围 4「配置优先级」由本 change 闭合**(需求 5 测试范围最后一项);`mod config;` 未接线(dead_code 预期)已在 design D1 标注;无 `api_key` 字段已在 design D5 标注;真实路径 / home dep 留装配 change(design D6)
