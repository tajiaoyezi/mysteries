## Why

bootstrap(Option A)只交付了 OpenAI *归一化*,刻意把 live 传输与凭据一并推迟(bootstrap design D3)。真实调用 LLM 的前置是先**安全地拿到 API key** —— 这是技术方案 §12 第 3 步「凭据链(env + file)」的前半,也是紧随其后的 live transport change 的硬前置:`OpenAiProvider` 需要 key 才能构造 `Authorization` 头。先行交付凭据链,使下一个 transport change 直接消费 `CredentialChain`,而非临时 `env::var` 读 key(bootstrap 已否决的 Option B 路径)。

## What Changes

- 新建 `credential` 模块(`src/credential/mod.rs`);`src/main.rs` 增 `mod credential;`(仅声明,本 change **不接线**)。
- 引入 `CredentialSource` trait(§5.6):`fn resolve(&self, provider: &str) -> Option<SecretString>`,`Send + Sync`,同步;未命中返回 `None`,MUST NOT panic。
- `EnvCredentialSource`:provider 名 → 环境变量映射(1.0 装 `openai` → `OPENAI_API_KEY`);env 访问经**可注入的 lookup**(production 默认 `std::env::var`),以支持离线确定性单测。
- `FileCredentialSource { path }`:从**注入路径**的凭据文件按 `provider = key` 行解析(hand-parse,不引 `toml`);测试用 tempdir。
- `CredentialChain(Vec<Box<dyn CredentialSource>>)`:按序询问,返回首个 `Some`,约定 **env 优先再 file**,皆无 → `None`。
- 密钥一律以 `secrecy::SecretString` 承载:`Debug` 自动脱敏,从类型层面满足需求「API Key 不暴露、不入日志」。
- OAuth **仅留接口位**:注释占位 `// struct OAuthCredentialSource;` + doc 说明 2.0 落地,本 change 不实装、不入 spec(无可验证行为)。

**明确不含**(留后续 change):

- live OpenAI 传输:reqwest + SSE 累积 + 超时/重试 + `ProviderError::{Auth, RateLimited, Timeout}` —— 紧随的 `add-openai-live-transport` change,消费本 change 的 `CredentialChain` 构造 provider auth 头。
- 配置分层(TOML user/project)与凭据文件的**最终路径/schema 归属** —— 留 config/assembly change;本 change 的文件格式为 **provisional**,仅满足 `CredentialSource` 契约,不抢定义权。
- 把凭据链接进 `main`/装配 —— 无消费者前不接线;与仓库「先 seam 后 consumer」先例一致(bootstrap 的 `wire` / `DeltaSink` 亦先交付后被消费)。`mod credential;` 在本 change 为 dead_code(警告为预期)。
- Anthropic 凭据(`ANTHROPIC_API_KEY`)—— 接口已能容纳(`resolve(provider)` 带 provider 入参),但 1.0 只装 `openai` 映射。

本 change 不触及 UI,故不涉及 `设计规范/` 引用(proposal/specs/tasks 的 UI 规则均不适用)。

## Capabilities

### New Capabilities

- `credential-source`: 按 provider 名解析 API key 的凭据接入层 —— `CredentialSource` trait、`EnvCredentialSource` / `FileCredentialSource` 两个来源、`CredentialChain`(env 优先再 file)优先级解析,以及 `secrecy::SecretString` 的密钥脱敏保证。

### Modified Capabilities

<!-- 无。本 change 仅新增 credential-source 能力,不改既有 capability 的任何 requirement。§9 的 ProviderError 错误变体随 add-openai-live-transport change 引入,不在此。 -->

## Impact

- **新增代码**:`src/credential/mod.rs`;`src/main.rs` 增 `mod credential;`(仅声明,不接线)。
- **新增依赖**:`secrecy = "0.10.3"`(§11 已列;`SecretString` Debug 脱敏);默认 features 即可,不开 `serde`(凭据不做序列化)。无 reqwest / SSE(随 transport change)。
- **构建/测试**:`cargo build` 通过(`credential` 模块 dead_code 警告为预期,无 consumer);凭据链属 headless 内核走**强制 TDD**(§5.6),`cargo test` 全绿且**离线** —— 注入假 env lookup + tempdir,不依赖真实 env / FS 状态。
- **下游契约**:确立 `CredentialSource` / `CredentialChain` 形状;`add-openai-live-transport` 在其上取 key 构造 `Authorization` 头,并把「凭据缺失 → §9 致命」接成 end-to-end 路径。
- **现状影响**:对既有 `main` 单轮(MockProvider)路径**零影响**——不接线、不改既有行为。
