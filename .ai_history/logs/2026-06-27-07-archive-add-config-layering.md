# 2026-06-27 · 07 · archive add-config-layering

## 决策

- **配置分层拆出独立 change、config 先行**(装配留 `add-cli-assembly`)| 主导:用户拍板拆分,子 agent propose、主 agent 审 | 依据:§12 step3 后半(凭据链已交付,配置分层补齐)
- **两类型:`RawConfig`(全 `Option`)+ `Config`(resolved 非 `Option`)** | 弃:`Config` 全 `Option` 下游每读兜底(把「是否已设」泄漏到全消费点)| 依据:D2 / §7
- **`merge` 字段级 + provider 嵌套递归**(`project.or(user)`:Some 覆盖、None 继承)| 对齐 §7「字段级 merge,非整文件替换」;§10 范围 4 核心被测点 | 依据:D3
- **`resolve` 默认 + 必填校验**:`max_iterations=8` / `timeout_secs=60` 文档化默认;`model` 与 `provider.kind` 缺失 → `ConfigError` 致命(§9),**不静默兜底** | 弃:全字段兜默认(隐藏 misconfig)| 依据:D4
- **无 `api_key` 字段**,凭据只走 `CredentialChain`;不 `deny_unknown_fields`(容忍前向字段)| 依据:D5 / §7
- **纯逻辑、零 IO / 零 home dep**:仅 `parse(&str)` / `merge` / `resolve` 吃 fixture 字符串 → 100% 离线,闭合 §10 范围 4;真实路径 / 读文件 / home dep 留装配 change | 依据:D6
- **显式偏离 §4(均标注)**:D1 单文件 `mod.rs`(弃 `schema.rs` 拆分,YAGNI)+ `mod config` 不接线(dead_code 预期);D7 `ConfigError` 置 config 模块内(module-local error 先例 = `ToolRegistryError` / `TransportErrorKind`)
- **审查修正**:原 design D7 误引「credential 先例自带类型」(credential 实为返回 `Option`、无 error 类型)→ 主 agent 审查纠正为 `ToolRegistryError`(`tool/mod.rs`)/ `TransportErrorKind`(`openai.rs`)| 主导:主 agent review
- **闭合 §10 范围 4「配置优先级」** —— 需求 5 个测试范围最后一项落点

## 变更

- 新增 `src/config/mod.rs`(类型 + `parse` / `merge` / `resolve` + 9 离线测);`main.rs` += `mod config;`(不接线,dead_code 预期);`Cargo.toml` += `toml = 0.8`(`Cargo.lock` 解析 0.8.23)
- 验证:`cargo test` 82 passed / 1 ignored;`cargo clippy` EXIT 0(仅 dead_code 预期);`fmt --check` 通过;`validate --strict` 通过
- archive:`changes/add-config-layering` → `changes/archive/2026-06-27-add-config-layering`;`specs/` 新增 `config-layering`(3 requirements)

## 待决

- **`add-cli-assembly`(紧随)**:`Config` 接 main 装配(按 `kind` 选 provider、读 `model` / `max_iterations` / `timeout_secs`);落定默认路径(`~/.config/mysteries/config.toml` / `./mysteries.toml`)+ `FileCredentialSource` 默认凭据文件路径(home dep 按需);stdin y/n decider;`lib.rs` 拆分 + `tests/` 端到端
- `DEFAULT_MAX_ITERATIONS` / `DEFAULT_TIMEOUT_SECS` 取值后续可微调;`config/schema.rs` 体量增大再拆(D1)
- `ProviderKind::Anthropic` 作配置值但无实装(装配 change 选中报 unsupported)
- (极小)design D7 文字 `MissingField(&'static str / String)` vs code `&'static str` —— 文档措辞,未返工

## 引用

- change:`add-config-layering`(rationale / rejected alternatives 全量见 design.md D1–D8;archive 路径 `changes/archive/2026-06-27-add-config-layering`)
- 技术方案 §4 / §7 / §9 / §10 范围 4 / §12 step3
- 前置 change:`add-credential-chain`(决策记录 2026-06-27-05)、`add-openai-live-transport`(2026-06-27-06)
- session log:无专属 checkpoint —— 子 agent propose + implement;主 agent review(核 §7 对齐、抓 D7 误引并令修正)+ commit / archive
