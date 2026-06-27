## Context

bootstrap 采 Option A,只做 OpenAI 归一化,把 live 传输与凭据一并推迟到「下一个 change」(bootstrap design D3)。用户已拍板把这两半**拆开、credential 先行**:本 change 交付凭据链(技术方案 §12 第 3 步前半,§5.6),作为紧随的 `add-openai-live-transport` change 的前置 —— 后者取 key 构造 `Authorization` 头,凭据缺失走 §9 致命路径。

约束:Rust 实现、核心能力自实现(禁第三方 Agent SDK);凭据链属 CLAUDE.md「强制 TDD」的 headless 内核;测试**不依赖真实网络 / FS / env 状态**;权威次序 code / 编译器 / 测试 > spec > Agent 推断,冲突显式标注。本 change 不触及 UI。

## Goals / Non-Goals

**Goals:**

- 确立 `CredentialSource` trait 与 `EnvCredentialSource` / `FileCredentialSource` / `CredentialChain` 的契约形状(§5.6)。
- 凭据链优先级语义:env 优先再 file,首个 `Some` 短路,皆无 → `None`。
- 用 `secrecy::SecretString` 承载密钥并以单测钉死 Debug 脱敏(满足「key 不暴露」)。
- 全程离线 TDD:注入假 env lookup + tempdir,确定性、零真实 IO。

**Non-Goals(留后续 change):**

- live OpenAI 传输:reqwest / SSE 累积 / 超时 / 重试 / `ProviderError::{Auth, RateLimited, Timeout}`(随 `add-openai-live-transport`)。
- 配置分层(TOML user/project)与凭据文件路径 / schema 的最终归属(随 config change)。
- 把凭据链接进 `main` / 装配(无消费者前不接线)。
- Anthropic provider 映射、OAuth 实装。

## Decisions

- **D1 新建 `credential` 模块,先 seam 后 consumer。** `src/credential/mod.rs`;`src/main.rs` 加 `mod credential;` 仅为让模块进编译与测试,本 change **不接线**,故 `credential` 为 dead_code(警告为预期)。此与仓库先例一致:bootstrap 即先交付 `wire::serialize_request` / `parse_response` 与 `DeltaSink` 为 dead_code,后续 change 才消费。备选:塞进 `provider/`(弃:凭据非 provider 子概念,§5.6 独立列出,放一起会把两个关注点耦在一个模块)。

- **D2 `CredentialSource` 同步 trait,签名严格采 §5.6。** `fn resolve(&self, provider: &str) -> Option<SecretString>` + `Send + Sync`。备选:`async`(弃:env 读与小凭据文件读同步即可,引 `#[async_trait]` 是无收益的复杂度,YAGNI)。

- **D3 `resolve` 带 `provider: &str` 入参,一个 source 内部按 provider 分派。** 而非「每 provider 一个 source 实例」。`EnvCredentialSource` 持 provider→env 名映射(1.0 仅 `openai`),`FileCredentialSource` 按文件行匹配 provider。这样为 Anthropic 等留位**而不新增类型**;权威采 spec §5.6 给定的签名。

- **D4 `secrecy::SecretString` 承载密钥。** Debug 自动脱敏满足需求「API Key 不暴露、不入日志」;后续 transport 取 key 经 `expose_secret()` 显式解封,使「明文出现点」集中可审计。备选:明文 `String`(弃:极易随 Debug / log / 错误串泄漏,直接违需求)。仅用默认 features,不开 `serde`(凭据不做序列化)。

- **D5 测试可注入 / 隔离,不碰真实 env / FS 状态(守 CLAUDE.md TDD)。**
  - `CredentialChain` 优先级逻辑 —— 用 in-test 假 `CredentialSource` 驱动(命中 / 未命中可编排),确定性、零 IO;这是本 change 唯一「有分支的逻辑」,重点测它。
  - `EnvCredentialSource` 的环境读取 —— 经注入的 lookup(形如 `Fn(&str) -> Option<String>`,production 默认 `std::env::var(..).ok()`);测试注入假 map。
  - `FileCredentialSource { path }` —— 路径注入,测试用 `tempfile` 写临时凭据文件后断言。
  备选:`EnvCredentialSource` 直读真实 env、测试用 `std::env::set_var`(弃:cargo 默认并行跑测,进程级 env 突变有竞态,且违「不依赖真实 env 状态」;`set_var` 在新 edition 亦趋 unsafe)。

- **D6 凭据文件格式 = `provider = key` 行(hand-parse),标注 provisional。** trim 首尾空白、跳过空行与 `#` 注释行,按首个 `=` 切分。不引 `toml`。**显式标注**:配置分层 change 可能改用 TOML 或重定文件路径 / 归属;本 change 只需满足 `CredentialSource` 契约,不预先固化凭据文件 schema、不与未来 config 抢定义权。备选:此刻就上 TOML(弃:提前引依赖 + 抢 config schema 定义权,投机,违 simplicity)。

- **D7 `CredentialChain(Vec<Box<dyn CredentialSource>>)` + inherent `resolve`。** newtype 采 §5.6;提供与 trait 同形的 inherent `resolve`(按序首个 `Some`)。本 change **不**让 `CredentialChain` 自身 impl `CredentialSource`(无嵌套链需求,YAGNI;留作未来低成本可加)。

- **D8 OAuth 仅注释占位,不入 spec。** `// struct OAuthCredentialSource; // 2.0 落地` + doc 注释;不出 trait impl、不出 spec requirement(无可验证行为,spec 只收可测场景)。§5.6 / §13 已记其接入方式 = 再加一个 `CredentialSource` 实现,配合 1.2 存 token。

## Risks / Trade-offs

- **[一个 change 交付 consumer-less 模块]** `credential` 本 change 无人调用 → 缓解:与仓库「先 seam 后 consumer」先例一致(bootstrap 的 `wire` / `DeltaSink`);紧随 transport change 即消费;dead_code 警告为预期、非缺陷。
- **[凭据文件格式 provisional]** 现定 `provider = key` 行,config change 可能改 → 缓解:D6 已显式标注;当前无外部依赖该格式,改动面被隔离在本模块内。
- **[env 全局状态使测试脆弱]** → 缓解:D5 注入 lookup 彻底回避真实 env;假 source 驱动链逻辑。
- **[secrecy 版本 API 漂移]**(`SecretString::new` vs `from`、`expose_secret` 签名随版本变) → 缓解:实现期 pin 版本(task 1.1 记由),并以「Debug 不含明文」这一**版本无关的契约**单测钉死行为,而非绑某版本 API 细节。

## Migration Plan

纯新增,greenfield 模块,无既有系统、无数据 / 接口迁移。不接线、不改 `main` 既有单轮路径,对现有行为零影响。回滚 = revert 本 change 的提交。

## Open Questions

- 凭据文件的**最终路径与格式归属**(本 change provisional vs config change 正式定)—— 留 config/assembly change 决断。
- `EnvCredentialSource` 注入 lookup 的**具体形状**(闭包 `Box<dyn Fn>` vs 小 trait vs 泛型参数)—— 实现期 TDD 收敛即可,对外契约(`resolve` 签名 + 离线可测)不变。
- 紧随的 `add-openai-live-transport` 中,「凭据缺失 → §9 致命」的**判定点**落在 provider 构造期还是装配期 —— 属 transport change 范围,在此仅记其依赖本 change 的 `CredentialChain`。
