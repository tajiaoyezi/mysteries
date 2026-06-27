## 1. cargo 脚手架

- [x] 1.1 创建 `Cargo.toml`(binary crate,edition 2021;deps:`tokio`[`rt-multi-thread`,`macros`]、`async-trait`、`serde`[`derive`]、`serde_json`、`thiserror`)
- [x] 1.2 建立模块骨架:`src/main.rs`、`src/error.rs`、`src/provider/{mod,wire,mock}.rs`、`src/agent/{mod,message}.rs`(`agent/mod.rs` 仅 `pub mod message;`)
- [x] 1.3 验证:`cargo build` 通过、`cargo run` 可启动(占位输出)

## 2. 内核规范类型(契约)

- [x] 2.1 定义 `Message`(System/User/Assistant{text,tool_calls}/ToolResult{call_id,content,is_error},§5.5)+ `ToolCall{id,name,arguments:Value}`(§5.1);derive:`Message` = `Serialize`/`Deserialize`/`Debug`/`PartialEq`,`ToolCall` = 同上 + `Clone`(因 `ModelResponse: Clone` 传递要求)
- [x] 2.2 定义 `ModelRequest{model,messages,max_tokens}`(省略 §5.1 `tools` 字段,见 design D5;derive `Debug`/`PartialEq`)、`ModelResponse{text,tool_calls,finish_reason}`(derive `Clone`/`Debug`/`PartialEq`,`Clone` 供 Mock 按 cursor 克隆脚本返回)、`FinishReason`(`Stop`/`Length`/`ToolCalls` + 未知/缺失兜底 `Other(String)`,见 design D12;derive `Clone`/`Debug`/`PartialEq`)
- [x] 2.3 定义 `ProviderError`(`thiserror`:`Transport`(§9 已列)+ `Decode`(§9 未列,显式增补),见 design D9)
- 注:本组为纯结构体/枚举,无逻辑,不走 red-green;其正确性由 §4、§6 的测试间接钉死。

## 3. Provider / DeltaSink trait(强制 TDD · 停点)

- [x] 3.1 【红 · 停点】写 `Provider` / `DeltaSink` 契约测试(in-test 最小假实现 + `Box<dyn Provider>` 调用、`DeltaSink::on_text` 捕获 + no-op sink),运行确认失败(失败原因正确,非编译噪声);**贴出 trait 草案 + 失败输出,停下等用户确认**(CLAUDE.md 折中档:新 trait 首次成型)
- [x] 3.2 【绿】定义 `Provider`(`#[async_trait]`,`name` / `complete(req, sink)`)与 `DeltaSink`(`on_text`),最小实现让 3.1 通过
- [x] 3.3 【重构】保持绿,清理

## 4. OpenAI 协议归一化 wire(强制 TDD)

- [x] 4.1 【红】写序列化测试:System/User/Assistant(带 tool_calls)/ToolResult → OpenAI messages(`role` 正确、`tool_call_id` 正确回填),确认失败
- [x] 4.2 【绿】实现 `wire::serialize_request`(`Message[]` → OpenAI 请求体;`ToolResult` 仅发 `content`,`is_error` 为内部簿记不入 wire;`Assistant` 空 `text` + `tool_calls` 时 `content` 发 `null`)
- [x] 4.3 【红】写解析测试:纯文本响应(→ `finish_reason=Stop`)、含 tool_calls(`function.arguments` JSON 字符串 → `Value`,→ `ToolCalls`)、`finish_reason` 映射(含未知值 → `Other`)、非法响应体 → `ProviderError`,确认失败
- [x] 4.4 【绿】实现 `wire::parse_response`(含 `arguments` 字符串解析与错误路径)
- [x] 4.5 【重构】抽公共、清理;`cargo test` 全绿

## 5. MockProvider(TDD)

- [x] 5.1 【红】写 Mock 测试:按脚本顺序返回、记录收到的 `ModelRequest`、脚本耗尽 → `ProviderError`(不 panic)、经 `DeltaSink` 吐增量,确认失败
- [x] 5.2 【绿】实现 `MockProvider{script,cursor,recorded}`,让 5.1 通过
- [x] 5.3 【重构】清理

## 6. 单轮 stdout 链路(conversation,TDD)

- [x] 6.1 【红】写 `run_single_turn(provider, prompt, sink)` 测试:用 `MockProvider` 断言(a)组装的 `ModelRequest.messages` 含 System+User、(b)返回回复文本、(c)sink 收到增量,确认失败
- [x] 6.2 【绿】实现 `run_single_turn`(IO 无关核心逻辑)
- [x] 6.3 实现 `main.rs` 薄胶水:读 prompt(`env::args` / stdin,无 clap)→ 装配 `MockProvider` + `StdoutSink` → 调 `run_single_turn`(可见输出由 `StdoutSink` 流式完成,返回值仅供测试,main **不重复打印**,至多补结尾换行);main 错误用 `Result<(), ProviderError>`;`cargo run "<prompt>"` 冒烟通过(main 不走 red-green,见 design D10)

## 7. 收尾

- [x] 7.1 `cargo build` 通过、`cargo test` 全绿、`cargo fmt`(可选 `cargo clippy`)
- [x] 7.2 自检:§5.1/§5.5/§10 与本 change 两个 spec 的 ADDED requirements 全部有测试落点;deviation(`ModelRequest.tools` 省略)已在 design D5 标注
