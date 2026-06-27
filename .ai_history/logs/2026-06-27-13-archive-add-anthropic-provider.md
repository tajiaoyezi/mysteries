# 2026-06-27 · 13 · archive add-anthropic-provider

## 决策

- **补 Anthropic provider:第二个 wire format 归一化到同一 ModelRequest/ModelResponse/ToolCall**,兑现 §13「换实现非改架构」| 主导:step5 收尾首项(provider 抽象兑现)| 依据:§5.1 / §5.2 / §5.5 / §13;以 add-openai-live-transport 为模板
- **D1 共享 `provider/transport.rs`(behavior-preserving 提取)**:with_retry/RetryPolicy/classify/backoff + 新 `SseAccumulator` trait + 泛型 `accumulate_stream`;openai 重构复用、**既有测试零回归 = 验收闸** | 弃 Anthropic 从 `openai::*` import(语义耦合错乱)
- **D2 `classify` 加 provider label**:映射不变、仅消息串用 label;**openai 旧串本就带 "OpenAI" 前缀** → `classify("OpenAI")` 逐字复现(零回归非循环,主 agent git diff 证)+ 强化为 exact assert 锁定
- **D3 anthropic_wire**:System→顶层 `system`(多条 join)、Assistant.tool_calls→`tool_use` 块、ToolResult→user 的 `tool_result` 块(`tool_use_id`)、tools 用 `input_schema`、`max_tokens` 必填(缺→默认 1024)
- **D4 anthropic_stream(impl SseAccumulator)**:event/data 解析、content_block_delta(`text_delta`→sink / `input_json_delta` 按 index 累积)、message_delta(stop_reason)、message_stop→落 ModelResponse;tool_use 输入串拼后 parse(非法→Decode);stop_reason 映射 FinishReason;归一化到**与 OpenAI 同一** ModelResponse/ToolCall
- **D5 AnthropicProvider**:`x-api-key` + `anthropic-version` 头、POST `/v1/messages`、cred 缺→Auth(离线测)
- **D6 各 +1 arm**:EnvCredentialSource `anthropic→ANTHROPIC_API_KEY`;select_provider Anthropic arm→真实(UnsupportedProvider 退役)
- **D7 gated `#[ignore]` smoke**(ANTHROPIC_API_KEY 缺早退,绝不进默认 test)
- **capability**:anthropic-transport NEW + credential-source MODIFIED(additive,+anthropic,openai 不变)+ cli-runtime MODIFIED(Anthropic arm unsupported→real);**provider-abstraction 不动、openai-transport 无 spec 变更**(纯 behavior-preserving 重构)
- **停点 task ①**(提取 + 零回归):主 agent 审 git diff 坐实零回归属实(旧 openai classify 本就产 "OpenAI HTTP status" 等、label 化逐字复现、映射不变、129 绿、Cargo 无 diff)
- **fixture 取 Anthropic 官方 streaming docs 样例**(`toolu_…`/`get_weather`/`San Francisco`)→ 化解「fixture 自写一致但错」风险,离线测对真格式;gated smoke 兜底真实端点
- **里程碑**:双 provider(OpenAI + Anthropic)归一化;§13 兑现

## 变更

- 新增 `provider/{transport,anthropic,anthropic_wire,anthropic_stream}.rs`;改 `provider/{mod,openai,stream}.rs`(提取复用)、`credential/mod.rs`(+anthropic arm)、`app.rs`(select_provider arm)
- 验证:`cargo test` 137 passed / 2 ignored(openai 零回归 openai 11 + stream 4);`clippy --all-targets` 零警告;`fmt` 通过;**零新依赖**(`Cargo` 无 diff)
- archive:`changes/add-anthropic-provider` → `changes/archive/2026-06-27-add-anthropic-provider`;`specs/` anthropic-transport NEW(3 ADDED)+ cli-runtime / credential-source 各 ~1 modified

## 待决

- **step5 剩**:内置命令(C8/C9,`/help /clear /model /status /exit` …)、流式打磨 / 超时 / 重试微调 —— 1.0 收尾仅剩此
- `ToolOutcome.exit`(工具卡 exit foot)、`anthropic-version` 可配、`max_tokens` 随 config 可调、§5.1 `tool_mode` 本地降级(Anthropic 原生 tool use 不需)

## 引用

- change:`add-anthropic-provider`(rationale / rejected alternatives 全量见 design.md D1–D7;archive 路径 `changes/archive/2026-06-27-add-anthropic-provider`)
- 技术方案 §5.1 / §5.2 / §5.5 / §9 / §13
- Anthropic Messages streaming docs(SSE event 形态 / 官方 fixture 来源)
- 前置 change:`add-openai-live-transport`(决策记录 06,模板)、`add-cli-assembly`(08,cli-runtime)、`add-credential-chain`(05,credential-source)
- session log:无专属 checkpoint —— 子 agent propose + implement(停点 task ① 提取零回归);主 agent review(git diff 坐实零回归非循环、anthropic wire/SSE 归一化对官方样例、3 个 capability delta 最小、fixture 官方来源)+ commit / archive
