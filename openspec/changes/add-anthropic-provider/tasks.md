## 1. 提取共享 transport.rs(重构 · openai 零回归闸)

- [x] 1.1 新建 `provider/transport.rs`:搬入 `with_retry` / `RetryPolicy` / `ErrorClassification` / `TransportFailure`·`TransportErrorKind` / `backoff_delay` / `classify_reqwest_error`;`classify(failure, provider_label)` 加 label 参数(见 design D1/D2);新增 `SseAccumulator` trait(`push_chunk`/`finish`)+ 泛型 `accumulate_stream<S,B,E,A: SseAccumulator>`
- [x] 1.2 重构 `openai.rs`/`stream.rs`:import 共享层,`classify(.., "OpenAI")`,`StreamAccumulator impl SseAccumulator`,`complete` 用泛型 `accumulate_stream`
- [x] 1.3 【验收闸】**既有 openai-transport 全部测试保持绿**(重构 behavior-preserving、错误串逐字不变);`cargo test` 全绿

## 2. Anthropic wire 序列化(强制 TDD)

- [x] 2.1 【红】写序列化测试:System→顶层 `system`;User/Assistant→`messages`;`Assistant.tool_calls`→`tool_use` 块;`ToolResult`→`user` 的 `tool_result` 块(`tool_use_id` 回填);`tools`→`input_schema`;`max_tokens` 出现;确认失败
- [x] 2.2 【绿】实现 `anthropic_wire::serialize_request`(纯函数,§5.1/§5.5,见 design D3;`max_tokens` 缺则默认常量)
- [x] 2.3 【重构】清理

## 3. Anthropic SSE 累积(强制 TDD · fixture 字节)

- [x] 3.1 【红】写累积测试(fixture Anthropic SSE 字节):`text_delta`→sink 即时、`input_json_delta`→按 index 累积、`message_stop`→`ModelResponse`、跨 chunk 缝合、非法 tool 输入→`Decode`;断言归一化为**与 OpenAI 同一** `ModelResponse`/`ToolCall`;确认失败
- [x] 3.2 【绿】实现 `AnthropicAccumulator`(impl `SseAccumulator`;event+data 解析、block index、stop_reason→FinishReason,见 design D4)
- [x] 3.3 【重构】清理

## 4. AnthropicProvider(TDD 离线部分)

- [x] 4.1 【红】写离线测试:cred 缺失(空/None CredentialChain)→ `complete` 立即 `Err(Auth)` 不触网;请求构造含 `x-api-key`+`anthropic-version` 头、`/v1/messages` URL;确认失败
- [x] 4.2 【绿】实现 `AnthropicProvider`(持 base_url+CredentialChain+client+RetryPolicy;`complete`:`resolve("anthropic")` 缺→Auth;头/url;`with_retry` 包裹;`accumulate_stream` + `AnthropicAccumulator`;`name()="anthropic"`,见 design D5);`provider/mod.rs` 注册新模块
- [x] 4.3 【重构】清理;`cargo build`

## 5. 凭据 anthropic arm(TDD)

- [x] 5.1 【红】写测试(注入 lookup):`resolve("anthropic")` 命中 `ANTHROPIC_API_KEY`→Some、未设→None;确认失败
- [x] 5.2 【绿】`EnvCredentialSource::resolve` 加 `"anthropic" => "ANTHROPIC_API_KEY"` arm(见 design D6)
- [x] 5.3 【重构】清理

## 6. select_provider 接线(TDD 离线)

- [x] 6.1 【红】写测试:`config.provider.kind = Anthropic` → `select_provider` 返 `Ok`(真实 `AnthropicProvider`,`name()=="anthropic"`),构造不触网;确认失败(现为 `Err(UnsupportedProvider)`)
- [x] 6.2 【绿】`app::select_provider` 的 `Anthropic` arm 改构造 `AnthropicProvider`(base_url 有则 new 无则 default,见 design D6)
- [x] 6.3 【重构】清理

## 7. gated live smoke + 收尾

- [x] 7.1 写 `#[ignore]` live 测试:`ANTHROPIC_API_KEY` 缺则早退;否则真实 `AnthropicProvider` + 真实 model → `complete` 断言非空文本 / sink 收增量;默认 `cargo test` 不跑它
- [x] 7.2 收尾:`cargo build`、`cargo test` 默认全绿且**不触网**(wire/SSE/classify/retry/cred/arm 全离线)、`cargo fmt`;自检:`anthropic-transport` ADDED + `credential-source`/`cli-runtime` MODIFIED requirements 全有落点(red-green / #[ignore] 已分类);**openai 测试零回归**已验;偏离已标注(max_tokens 默认、anthropic-version pin、tool_mode 不含)
