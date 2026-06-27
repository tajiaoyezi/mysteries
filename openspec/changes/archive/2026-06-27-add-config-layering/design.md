## Context

技术方案 §12 第 3 步 = 配置分层 + 凭据链;凭据链已由 `add-credential-chain` 交付。本 change 补齐配置分层(§7),并闭合 §10 范围 4「配置优先级」—— 需求 5 个测试范围里唯一还没落点的一项。`Config` 是紧随的「main 接 Loop」装配 change(`add-cli-assembly`)的前置:装配按 `provider.kind` 选 provider、读 `model` / `max_iterations` / `timeout_secs`。用户已拍板**拆「config 分层 / main 装配」两个 change,config 先行**。

约束:Rust 自实现;config merge 属 CLAUDE.md「强制 TDD」的 headless 内核;测试**不依赖真实 FS / 网络**(用 fixture 字符串);权威次序 code / 编译器 / 测试 > spec > Agent 推断,冲突显式标注。本 change 不触及 UI。

## Goals / Non-Goals

**Goals:**

- `Config`(运行用,非 `Option`)+ `RawConfig`(分层用,全 `Option`)类型,无 `api_key`(凭据走 `CredentialChain`)。
- `parse`(TOML → `RawConfig`)+ `merge`(字段级、project 覆盖 user、`None` 继承,含 provider 嵌套)+ `resolve`(默认 + 必填校验 → `Config`)。
- 离线 TDD 闭合 §10 范围 4(两份 fixture 字符串)。

**Non-Goals(留装配 change):**

- 真实默认路径解析、home-dir 依赖、读文件 loader(本 change 零 IO / 零 home dep)。
- 把 `Config` 接进 main / 装配、按 config 选 provider、`FileCredentialSource` 默认路径落定。
- TUI、Anthropic provider 实装、`tool_mode` 降级、内置命令。

## Decisions

- **D1 单文件模块。** `src/config/mod.rs` 容纳类型 + `parse` / `merge` / `resolve` + 测试;§4 列了 `config/schema.rs`,但本 change 体量小,单文件更简(YAGNI;日后膨胀再拆 `schema.rs`)。`src/main.rs` 加 `mod config;` 仅为入编译 / 测试,**不接线** → dead_code(与 credential `wire` 先例一致,警告为预期)。

- **D2 两类型:`RawConfig`(全 `Option`)+ `Config`(resolved)。** 分层 merge 需要「未设置」语义,故 parse / merge 在 `RawConfig`(每字段 `Option`)上做;`resolve` 落成 `Config`(§7 给定的非 `Option` 运行类型,`model: String` / `max_iterations: u32` / `timeout_secs: u64`)。备选:`Config` 直接全 `Option`、用处再兜默认(弃:把「是否已设」泄漏到全下游消费点,B 每次读都要兜底)。

- **D3 `merge` 字段级 + provider 嵌套递归。** `merge(user, project)`:project 的 `Some` 覆盖、`None` 继承;`provider` 为嵌套 `RawProviderConfig` 时同样逐字段 merge(非整表替换)—— 对齐 §7「字段级 merge,不是整文件替换」。这是本 change 的核心逻辑、§10 范围 4 的被测点。

- **D4 `resolve` 默认 + 必填校验。** `max_iterations` / `timeout_secs` 未设 → 文档化默认常量(`DEFAULT_MAX_ITERATIONS = 8`、`DEFAULT_TIMEOUT_SECS = 60`,均可后续调);`model` 与 `provider.kind` 仍缺失 → `ConfigError::MissingField`(§9 配置非法 = 致命,**不静默默认**,避免「跑起来连错 provider」)。备选:全字段兜默认(弃:model / provider 无安全通用默认,静默兜底=隐藏 misconfig)。

- **D5 无 `api_key` 字段。** `Config` / `RawConfig` / `ProviderConfig` 一律不含 `api_key`(§7 明示);凭据只走 `CredentialChain`。TOML 若误带 `api_key`,因类型无此字段而被忽略(不 `deny_unknown_fields`,容忍前向字段)。

- **D6 纯逻辑、零 IO / 零 home dep。** 本 change 只暴露 `parse(&str)` / `merge` / `resolve`(吃调用方给的字符串),**不**读真实文件、**不**解析 `~` / `$HOME`。真实「解析默认路径 → 读两份文件 → 调 parse/merge/resolve」的薄 loader 归装配 change(与现有 `main` 薄 IO 一致)。**理由**:A 100% 离线 / 确定性,§10 范围 4 用 fixture 字符串即可全覆盖;home-dep 不在本 change 引入。

- **D7 `ConfigError` 置于 `config` 模块内。** 不动 `error.rs`(§4 虽集中错误,但 module-local error 的真实先例是 `ToolRegistryError`(`tool/mod.rs`)+ `TransportErrorKind`(`openai.rs`);本 change 自包含、零跨文件改动)。`thiserror`:`Toml(String)`(解析失败)+ `MissingField(&'static str / String)`(resolve 必填缺失)。日后需要可上移 `error.rs`。

- **D8 serde 表示。** `ProviderKind` 用 `#[serde(rename_all = "lowercase")]` → TOML `kind = "openai" | "anthropic" | "mock"`;`AuthType` 用 `#[serde(rename_all = "snake_case")]` → `auth_type = "api_key"`;`RawConfig` 各字段 `#[serde(default)]` 使缺失 → `None`。具体 attribute 实现期以编译 / 测试钉死(权威 code)。

## Risks / Trade-offs

- **[consumer-less 一个 change]** `config` 本 change 无人读 → 缓解:与仓库「先 seam 后 consumer」先例一致(credential 亦如此);且本 change 闭合一个 **tracked 需求测试项**(§10 范围 4)+ 纯离线测,正当性强于单纯占位;紧随装配 change 即消费。
- **[默认常量取值]** `max_iterations = 8` / `timeout_secs = 60` 为判断值 → 缓解:文档化 + 可配(本就来自 config),装配 / 后续可调;不写死进 spec 场景(spec 只断言「未设 → 取默认」行为)。
- **[provider 嵌套 merge 易错]** 整表替换 vs 字段级 → 缓解:D3 明确字段级 + 专门测「project 只改 base_url、继承 kind」。
- **[Anthropic 作配置值但无实装]** `ProviderKind::Anthropic` 可被配置选中却无 provider → 缓解:本 change 不消费 config;装配 change 选中 Anthropic 时报 unsupported(其范围)。

## Migration Plan

纯新增模块,greenfield;无数据 / 接口迁移。不接线、不改 `main` 既有单轮路径,对现有行为零影响。回滚 = revert 本 change 提交。

## Open Questions

- 默认路径常量(`~/.config/mysteries/config.toml` / `./mysteries.toml`)与 `FileCredentialSource` 默认凭据文件路径的**最终落定**,归装配 change(本 change 不引 home dep)。
- `DEFAULT_MAX_ITERATIONS` / `DEFAULT_TIMEOUT_SECS` 的最终取值,可在装配 / 用后续微调。
- 是否需要 `config/schema.rs` 拆分:体量增大(如加更多 provider 字段)时再拆,本 change 单文件。
