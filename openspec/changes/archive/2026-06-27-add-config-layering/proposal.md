## Why

§10 的 5 个测试范围里,「配置优先级」(范围 4)是**唯一还没落点**的需求项 —— 主循环(范围 1)、工具调用 / 结果(范围 2)、权限确认 / 拒绝(范围 3)、Mock / 归一化(范围 5)已分别由 agent-loop / builtin-tools / credential 等 change 覆盖。§7 的两层 TOML 字段级 merge 是技术方案 §12 第 3 步的后半(凭据链已交付,配置分层补齐)。`Config` 也是紧随的「main 接 Loop」装配 change 的硬前置(装配按 config 选 provider、读 model / max_iterations / timeout)。本 change 先行交付**纯离线**的配置层,闭合该测试缺口,并给装配 change 一个可消费的 `Config`。

## What Changes

- 新建 `config` 模块(`src/config/mod.rs`);`src/main.rs` 加 `mod config;`(仅声明,本 change **不接线**)。
- 类型:`RawConfig`(serde,全 `Option` 字段表「未设置」)+ `Config`(resolved,非 `Option`)+ `RawProviderConfig` / `ProviderConfig`(`kind` / `base_url: Option<String>` / `auth_type`,**无 `api_key` 字段** —— 凭据走已就位的 `CredentialChain`)+ `ProviderKind { OpenAi, Anthropic, Mock }` + `AuthType { ApiKey }`(OAuth 占位)。
- `parse(&str) -> Result<RawConfig, ConfigError>`:TOML 解析,缺失字段 → `None`。
- `merge(user, project) -> RawConfig`:**字段级**,project 的 `Some` 覆盖、`None` 继承;含 `provider` 嵌套的字段级 merge。
- `resolve(RawConfig) -> Result<Config, ConfigError>`:`max_iterations` / `timeout_secs` 套默认常量;`model` 与 `provider.kind` 缺失 → `ConfigError`(§9 配置非法 = 致命)。
- `ConfigError`(`thiserror`:TOML 解析失败 / 缺必填字段),置于 `config` 模块内(自包含)。
- 离线单测**闭合 §10 范围 4**:两份 TOML fixture 字符串 → `merge` → 断言 `Some` 覆盖 / `None` 继承(含 `provider` 嵌套)。

**明确不含**(留装配 change `add-cli-assembly`):

- 真实默认路径解析(`~/.config/mysteries/config.toml` / `./mysteries.toml`)、home-dir 依赖、读文件的 loader —— 本 change 纯 `parse` + `merge` + `resolve`(吃 fixture 字符串),**零 IO、零 home dep**,故 100% 离线可测。
- 把 `Config` 接进 main / 装配、按 config 选 provider、`FileCredentialSource` 默认路径落定 —— 全归装配 change。
- TUI、Anthropic provider 实装(`ProviderKind::Anthropic` 本 change 仅作配置值,装配 change 选中它会报 unsupported)、§5.1 `tool_mode` 降级、内置命令。
- `mod config;` 在本 change 为 dead_code(无 consumer;与 credential 先例一致,警告为预期)。

本 change 不触及 UI,故不涉及 `设计规范/` 引用。

## Capabilities

### New Capabilities

- `config-layering`: 两层 TOML 配置接入 —— `Config` / `RawConfig` 类型(无 `api_key`)、TOML 解析、字段级分层 merge(project 覆盖 user、`None` 继承)、resolve 为运行配置(默认 + 必填校验)。

### Modified Capabilities

<!-- 无。本 change 仅新增 config-layering 能力,不改既有 capability 的任何 requirement。 -->

## Impact

- **新增代码**:`src/config/mod.rs`;`src/main.rs` 加 `mod config;`(仅声明,不接线)。
- **新增依赖**:`toml = "0.8"`(§11;当前 lock 解析 `0.8.23`;理由:通过 serde 将 TOML fixture 字符串反序列化为 `RawConfig`,仅此)。**无** home-dir crate(留装配 change 按需引,std env 优先)。
- **构建 / 测试**:`cargo build` 通过(`config` dead_code 警告为预期,无 consumer);config merge 属 headless 内核走**强制 TDD**(§7 / §10),`cargo test` 全绿且**离线**(fixture 字符串,无真实 FS)。**本 change 闭合需求 5 个测试范围的最后一项**(§10 范围 4)。
- **下游契约**:`add-cli-assembly` 消费 `Config` —— 按 `provider.kind` 选 provider、读 `model` / `max_iterations` / `timeout_secs`。
- **现状影响**:对既有 `main` 单轮(MockProvider)路径**零影响**(不接线)。
