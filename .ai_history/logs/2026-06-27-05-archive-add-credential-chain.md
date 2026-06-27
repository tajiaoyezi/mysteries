# 2026-06-27 · 05 · archive add-credential-chain

## 决策

- **transport + 凭据拆分:凭据先行**(`add-credential-chain`),transport 留紧随的 `add-openai-live-transport` | 选:凭据先做 | 弃:二者捆一个 change(主 agent 原议)、transport 先做 | 主导:子 agent 提拆分方案 → 主 agent 审查认可(凭据是 transport 硬前置、纯逻辑、可完全离线测,先做去风险)| 依据:change design.md Why + 仓库 2-split 先例
- **`CredentialSource` 同步 trait + `resolve(provider)` 单签名多 provider 分派** | 弃:async(YAGNI)、每 provider 一个 source 类型 | 依据:design D2 / D3、§5.6
- **密钥一律 `secrecy::SecretString`,Debug 脱敏,`expose_secret()` 为集中可审计解封点** | 弃:明文 `String`(易随 log/Debug 泄漏)| 依据:D4(满足需求「API Key 不暴露」)
- **离线测试**:注入 env lookup(避 `set_var` 竞态/unsafe)+ tempdir + 假 source 驱动链逻辑 | 依据:D5
- **凭据文件 `provider = key` 行,标注 provisional**,不抢未来 config 的 schema/路径定义权 | 依据:D6
- OAuth 仅注释占位、不入 spec(无可测行为)| 依据:D8
- 审查修正:`clippy::type_complexity` → 引入 `EnvLookup` 类型别名 | 主导:主 agent 审查发现

## 变更

- 新增 `src/credential/mod.rs`:`CredentialSource` / `EnvCredentialSource` / `FileCredentialSource` / `CredentialChain` + `secrecy` 脱敏 + OAuth 占位 + 离线测试;`main.rs` 加 `mod credential;`(未接线,dead_code 预期)
- 新依赖:`secrecy = 0.10.3`(pin 版本,§11)
- 验证:`cargo test` 58 passed;`cargo clippy` 仅剩 dead_code
- archive:`changes/add-credential-chain` → `changes/archive/2026-06-27-add-credential-chain`;`specs/` 新增 `credential-source`

## 待决

- **`add-openai-live-transport`(紧随)**:reqwest + SSE 累积 + 超时/重试 + `ProviderError::{Auth, RateLimited, Timeout}`;消费本 change 的 `CredentialChain` 构造 auth 头,凭据缺失 → §9 致命
- config 分层:定凭据文件**最终路径/schema** + 权限硬化(§5.6 chmod 600,本 change 未做)
- `main` 接 Loop + stdin y/n decider(transport/assembly 后)
- `credential` 模块在 transport 接入前为 dead_code

## 引用

- change:`add-credential-chain`(rationale / rejected alternatives 全量见 design.md D1–D8;archive 路径 `changes/archive/2026-06-27-add-credential-chain`)
- 技术方案 §5.6 / §11 / §12(step 3 前半)/ §13(OAuth 接入方式)
- 前置 change:`add-builtin-tools`(决策记录 2026-06-27-04)
- session log:无专属 checkpoint —— 子 agent propose / implement;主 agent 负责 review、拆分认可与 clippy 修正
