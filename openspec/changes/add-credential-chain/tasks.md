## 1. 模块骨架 + 依赖

- [x] 1.1 `Cargo.toml` 加 `secrecy`(选定版本并在 proposal/commit 记由,§11;默认 features,不开 `serde`);`cargo build` 通过
- [x] 1.2 建 `src/credential/mod.rs`;`src/main.rs` 加 `mod credential;`(仅声明,**不接线**);`cargo build` 通过(`credential` dead_code 警告为预期,见 design D1)

## 2. CredentialSource trait(强制 TDD · 停点)

- [x] 2.1 【红 · 停点】写 `CredentialSource` 契约测试:in-test 假实现 + `Box<dyn CredentialSource>` 调 `resolve`,命中 → `Some`、未命中 → `None`(不 panic);运行确认失败(失败原因正确,非编译噪声)。**贴出 trait 草案 + 失败输出,停下等用户确认**(CLAUDE.md 折中档:新 trait 首次成型)
- [x] 2.2 【绿】定义 `CredentialSource`(`Send + Sync`,`fn resolve(&self, provider: &str) -> Option<SecretString>`),最小实现让 2.1 通过
- [x] 2.3 【重构】保持绿,清理

## 3. EnvCredentialSource(TDD · 注入 lookup)

- [x] 3.1 【红】写测试(注入假 env lookup,不碰真实 env):`"openai"` → `OPENAI_API_KEY` 命中 → `Some`(`expose_secret()` 断言值)、变量未设置 → `None`;确认失败
- [x] 3.2 【绿】实现 `EnvCredentialSource`(provider→env 名映射含 `openai`;env 访问经注入 lookup,production 默认 `std::env::var(..).ok()`,见 design D5)
- [x] 3.3 【重构】清理

## 4. FileCredentialSource(TDD · tempdir)

- [x] 4.1 【红】写测试(`tempfile` 写临时凭据文件):含 `openai = sk-file` 行 → `resolve("openai")` 命中 `Some`;无匹配行 / 文件不存在 → `None`(不 panic);确认失败
- [x] 4.2 【绿】实现 `FileCredentialSource { path }`(`provider = key` 行 hand-parse:trim、跳过空行与 `#` 注释、按首个 `=` 切分,见 design D6)
- [x] 4.3 【重构】清理

## 5. CredentialChain 优先级(TDD)

- [x] 5.1 【红】写测试(in-test 假 source 编排命中/未命中):env 命中优先于 file、env 缺失回落 file、全部缺失 → `None`、命中即短路其余;确认失败
- [x] 5.2 【绿】实现 `CredentialChain(Vec<Box<dyn CredentialSource>>)` + inherent `resolve`(按序返回首个 `Some`,见 design D7)
- [x] 5.3 【重构】清理

## 6. 密钥脱敏保证 + OAuth 占位

- [x] 6.1 【测试】写脱敏 guard 测试:对持密钥的 `SecretString` 执行 `format!("{:?}")`,断言输出不含明文子串(呈 redaction 占位);明文仅经 `expose_secret()` 取得(characterization 测试,钉死「key 不暴露」契约,见 design D4)
- [x] 6.2 OAuth 接口位:`// struct OAuthCredentialSource; // 2.0 落地` 注释占位 + doc 说明,不实装、不入 spec(见 design D8)

## 7. 收尾

- [x] 7.1 `cargo build` 通过、`cargo test` 全绿且**离线**(无真实 env / FS 状态依赖)、`cargo fmt`(可选 `cargo clippy`)
- [x] 7.2 自检:§5.6 与 `credential-source` spec 的 5 条 ADDED requirements 全部有测试落点;`mod credential;` 未接线(dead_code 预期)已在 design D1 标注;凭据文件格式 provisional 已在 design D6 标注
