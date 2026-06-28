# Tasks — expose-token-usage

> TDD:provider 归一化属 headless 内核,**强制红-绿**。
> 🔴 **红灯停点①**:`Usage` 类型 + `ModelResponse.usage` 首次成型(新归一化类型)——测试写完、贴出**运行时**失败输出后**停下等确认**,再写绿。
> 红灯构造为「运行时红」而非编译错:先加 `Usage` + `usage` 字段的**最小桩**(如 `total()` 桩返回 `0`)使其编译,测试断言真实行为 → 运行时失败。

## 1. 类型与归一化(provider-abstraction,强制 TDD)

- [ ] 1.1 【红】先只写测:① 构造 `ModelResponse{ usage: Some(Usage{input_tokens, output_tokens}), .. }`,断言 `usage.total()` == input + output;② MockProvider 脚本返回带 `usage` 的 `ModelResponse`,`complete` 后断言 usage 原样透传;③ 无 usage 时 `ModelResponse.usage == None`。运行确认失败(桩 `total()` 返回 0 → 运行时红,非编译错)。
- [ ] 1.2 🔴 **红灯停点①**:贴出 1.1 测试代码 + 失败输出,**停下等确认**。
- [ ] 1.3 【绿】最小实现:`Usage{input_tokens, output_tokens}` + `total()`;`ModelResponse.usage: Option<Usage>`;`ModelResponse` / `FinishReason` 派生 `Default`(`FinishReason::default()=Stop`);修全仓库既有 `ModelResponse` 构造点补 `usage`(优先 `..Default::default()`)。Mock 经预置响应透传 usage(不新增 API)。
- [ ] 1.4 零回归:既有 provider-abstraction / agent-loop / 其它构造 `ModelResponse` 的测试补字段后**保持绿**。

## 2. OpenAI 用量解析(openai-transport,强制 TDD)

- [ ] 2.1 【红】先只写测:① fixture = 含末尾 usage-only chunk(`prompt_tokens` / `completion_tokens`)的 OpenAI SSE 字节流喂累积器,断言 `ModelResponse.usage == Some(Usage{input=prompt, output=completion})` 且 text/tool_calls/finish_reason 与无 usage chunk 时一致;② 离线构造请求,断言请求体含 `stream_options.include_usage == true`(与 `stream:true` 并存);③ 无 usage chunk 的流 → `usage == None`。运行确认失败。
- [ ] 2.2 【绿】最小实现:请求加 `stream_options.include_usage`;累积器识别 usage-only chunk(`choices` 空 + 带 `usage`)取值填 `usage`,不误当文本/工具增量;缺失 / 解析失败降级 `None`。
- [ ] 2.3 边界(连写不停):usage chunk 与 `[DONE]` 的先后顺序;`usage` 部分字段缺失。

## 3. Anthropic 用量解析(anthropic-transport,强制 TDD)

- [ ] 3.1 【红】先只写测:① fixture = 含 `message_start.usage.input_tokens` + `message_delta.usage.output_tokens` 的 Anthropic SSE,断言 `usage == Some(Usage{input, output})`,且与等价 OpenAI 响应的 usage 形状一致;② 无任何 usage 字段 → `None`。运行确认失败。
- [ ] 3.2 【绿】最小实现:累积器从 `message_start` 取 input_tokens、`message_delta` 取 output_tokens 合成;任一缺失记 0,均无则 `None`;解析失败降级 `None`,不影响主体归一化。
- [ ] 3.3 边界(连写不停):只有 `message_start` 无 `message_delta` usage 的情形。

## 4. 收尾验证

- [ ] 4.1 `cargo build` 通过;`cargo test` 全绿(含新红-绿)。
- [ ] 4.2 `openspec validate expose-token-usage --strict` 通过。
- [ ] 4.3 `cargo clippy --all-targets -- -D warnings` 零警告;`cargo fmt --check` 净。
