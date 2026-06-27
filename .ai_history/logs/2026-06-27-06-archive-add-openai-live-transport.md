# 2026-06-27 · 06 · archive add-openai-live-transport

## 决策

- **live OpenAI transport(拆分后半,transport-only)**:reqwest + SSE 累积 + 超时/重试 + §9 错误变体,**消费** `add-credential-chain` 的 `CredentialChain` | 主导:范围内自然项(凭据已先行交付)| 依据:change design.md + §5.1 / §5.2 / §9
- **§9 错误变体补全**:`ProviderError::{Auth(致命), RateLimited, Timeout}`(bootstrap D9 预告,现有构造点);`classify` 为纯函数、离线可测 | 依据:design
- **SSE 累积**(`StreamAccumulator`):文本即时推 sink、tool_call 按 index 累积、`[DONE]` 收尾、跨 chunk 边界缝合;fixture 字节流离线测 | 依据:§5.2
- **重试有界 + 指数退避**,用 `tokio` `test-util` mock 时间离线测时序 | 依据:design
- **密钥**:`expose_secret()` 仅在构造 `Bearer` 头一处,无任何 println/dbg/tracing 打印 key/请求 → 零泄漏面 | 依据:`secrecy`(add-credential-chain)
- live 调用仅 `#[ignore]` + 无 `OPENAI_API_KEY` 早退,默认 `cargo test` 不触网
- **审查修正 ①**:`with_retry` 收窄到只覆盖「send + 2xx」,流式循环移出 retry(`accumulate_stream` 单次消费,chunk 错误 → `Transport` 致命)→ 消除「中途断流重试 → sink 重复推文本」;新增 fake-stream 离线回归测试 | 主导:主 agent 审查发现 → 出修复 prompt
- ② nit:`classify` 5xx → `RateLimited` 语义略偏,行为(重试)对 —— 接受不改

## 变更

- 新增 `src/provider/openai.rs`(`OpenAiProvider` + `classify` + `with_retry` + `accumulate_stream`)、`src/provider/stream.rs`(`StreamAccumulator`);`error.rs` 加 3 变体;`provider/mod.rs` 注册 `openai`/`stream`;激活 `wire::serialize_request`
- 新依赖:`reqwest`(json/stream/rustls-tls)、`futures-util`;`tokio` += `test-util`(mock 时间测重试)
- 验证:`cargo test` 73 passed / 1 ignored;`cargo clippy` 仅 dead_code
- archive:`changes/add-openai-live-transport` → `changes/archive/2026-06-27-add-openai-live-transport`;`specs/` 按 change delta 同步

## 待决

- **main 接 Loop + 真实 OpenAiProvider 装配 + stdin y/n decider**(留 config/assembly change);现 `main` 仍单轮 Mock,`OpenAiProvider` 在接线前为 dead_code
- 配置分层(TOML user/project,定 base_url / model / 凭据文件最终归属)
- Anthropic provider;§5.1 本地非 function-calling 的 `tool_mode` 降级
- ② classify 5xx 语义(接受)

## 引用

- change:`add-openai-live-transport`(rationale / rejected alternatives 见 design.md;archive 路径 `changes/archive/2026-06-27-add-openai-live-transport`)
- 技术方案 §5.1 / §5.2 / §9 / §11
- 前置 change:`add-credential-chain`(决策记录 2026-06-27-05)
- session log:无专属 checkpoint —— 子 agent propose / implement;主 agent 负责 review(抓出 ① 流式重试 double-emit)、出修复 prompt 与 commit/archive
