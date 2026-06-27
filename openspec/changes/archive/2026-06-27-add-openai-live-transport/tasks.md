## 1. 依赖 + 错误变体

- [x] 1.1 `Cargo.toml` 加 `reqwest`(`default-features = false`,features `json` / `stream` / `rustls-tls`)+ `futures`(`StreamExt`;或 `futures-util`,取最小 features);选版本并记由(§11);`cargo build` 通过
- [x] 1.2 `error.rs` 加 `ProviderError::{Auth, RateLimited, Timeout}`(单元变体 + `#[error(...)]`,§9 文案),保持 `PartialEq` / `Eq`;`cargo build`(纯枚举增补,不走 red-green;构造点由 §3/§4/§5 测试钉死)

## 2. SSE 累积器纯逻辑(强制 TDD · 停点)

- [x] 2.1 【红 · 停点】写累积器测试(fixture 字节流):多文本 delta 经 sink 即时推、`tool_call` 按 `index` 累积(id/name/arguments 分片)、`[DONE]` → `ModelResponse`、**chunk 边界切断 SSE 事件仍正确缝合**、arguments 非法 JSON → `Decode`;运行确认失败(原因正确)。**贴出累积器接口草案 + 失败输出,停下等用户确认**(CLAUDE.md 折中档:新接口首次成型)
- [x] 2.2 【绿】实现 `stream.rs` 累积器(吃 `&[u8]`、内部行缓冲跨 chunk 缝合、自解 `data:` / `[DONE]`、忽略 `event:` / 注释行;`tool_call` `BTreeMap<usize, Partial>`;终态 arguments 拼接后 JSON parse,复用 wire 的非法→`Decode` 语义)
- [x] 2.3 【重构】保持绿,清理

## 3. HTTP 错误分类纯函数(TDD)

- [x] 3.1 【红】写 `classify` 测试:`401`/`403` → `Auth` 致命、`429` → `RateLimited` 重试、`5xx` → `RateLimited` 重试、`400`/`404` → `Transport` 致命、timeout kind → `Timeout` 重试、connect/网络 kind → 可重试 `Transport`、解析失败 → `Decode` 致命;确认失败
- [x] 3.2 【绿】实现 `classify`(吃 status `u16` / 抽象传输错误 kind → `{Retryable | Fatal}(ProviderError)`,见 design D5)
- [x] 3.3 【重构】清理

## 4. 超时 + 重试驱动(TDD · 虚拟时间)

- [x] 4.1 【红】写 `with_retry` 测试(注入脚本化 `attempt` + `tokio::time` 暂停推进):`RateLimited×2` then `Ok` → 3 次尝试成功、`Auth` → 1 次不重试、全 `RateLimited` → 耗尽返回最后错误、单次超时 → `Timeout` 触发重试、指数退避间隔随虚拟时间推进;确认失败
- [x] 4.2 【绿】实现 `with_retry`(per-attempt `tokio::time::timeout` 包 `attempt`;按 `classify` 决定重试;指数退避至 `max_retries`)
- [x] 4.3 【重构】清理

## 5. OpenAiProvider 装配(TDD · 离线部分)

- [x] 5.1 【红】写离线测试:(a) 请求体构造含 `wire::serialize_request` 归一化 messages + `model = req.model` + `"stream": true`、目标 URL = `{base_url}/chat/completions`;(b) 凭据缺失(`CredentialChain::new(vec![])` 或注入空 env lookup)→ `complete` 立即 `Err(Auth)`,不发 HTTP、不重试;确认失败
- [x] 5.2 【绿】实现 `OpenAiProvider`(持 `base_url` + `CredentialChain` + `reqwest::Client`;`complete`:`resolve("openai")` 缺失 → `Err(Auth)`;请求体 = `wire::serialize_request` + `stream:true`;`Authorization: Bearer` 经 `expose_secret()`;`reqwest` POST → `bytes_stream()` 经 `futures::StreamExt` 喂累积器;整体过 `with_retry`)。**激活** `wire::serialize_request`
- [x] 5.3 【重构】`provider/mod.rs` 注册 `openai` / `stream`;清理;`cargo build`(`wire::parse_response` 仍 dead_code 为预期,见 design D3)

## 6. gated live smoke

- [x] 6.1 写 `#[ignore]` live 测试:若 `OPENAI_API_KEY` 缺失则 early-return(skip);否则构造真实 `OpenAiProvider`(默认 `base_url`、`EnvCredentialSource`)+ `ModelRequest`(真实 model,如 env `OPENAI_MODEL` 或 `"gpt-4o-mini"`)→ `complete` 断言非空文本 / sink 收到增量。验证默认 `cargo test` **不**跑它,`cargo test -- --ignored` 才跑
- [x] 6.2 (可选)main 单轮接真实 provider:需 `run_single_turn` 接受 model(轻微 `conversation` 改),默认**不做**;如做须显式标注偏离并补测

## 7. 收尾

- [x] 7.1 `cargo build` 通过、`cargo test` 默认全绿且**不触网**(SSE / classify / retry / cred-missing 全离线)、`cargo fmt`(可选 `cargo clippy`)
- [x] 7.2 自检:§5.1 / §5.2 / §9 与 `openai-transport` 4 条 + `provider-abstraction` 错误分类 requirement 全部有测试落点;偏离已标注(D2 不持 model、D3 流式 only / `parse_response` 仍 dead、D5 `5xx → RateLimited` 命名折中);live smoke `#[ignore]` + env-guard 不进默认 test
