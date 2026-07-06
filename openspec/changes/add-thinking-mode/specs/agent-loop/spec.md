# agent-loop Delta

## ADDED Requirements

### Requirement: 每轮注入思考深度并回传思考块

`Agent` SHALL 提供 `set_thinking_depth(Arc<Mutex<Depth>>)` setter(字段默认 `Depth::Low`,MUST NOT 改 `Agent::new` 签名,仿 `set_permission_mode`)。`run_observed` 内**两处** `ModelRequest`(主循环 loop 顶、与循环外的 forced-final 收尾请求)SHALL 各读一次 depth 快照并填入 `ModelRequest.thinking = Some(ThinkingConfig{depth})`(forced-final 块在 for 外,MUST 重新读快照、MUST NOT 复用已出作用域的循环内局部变量;`Depth::Off` 也传,交由 wire 层按能力判 `None`/`disabled`)。**两处** `Message::Assistant` push(主循环与 forced-final 后)SHALL 各带上其轮 `response.thinking`,使下一轮请求经 wire 原样回传(满足 Anthropic 带 tool_use 多轮约束)。开启思考时 agent-loop MUST NOT 设置强制 `tool_choice`。`/model` 切换(`set_model`/`SetModel` 路径)SHALL 清空 caller `history` 内所有 `Message::Assistant.thinking`,以免跨模型 signature 在带 tool_use 多轮回传触发 400。

#### Scenario: depth 快照进请求

- **WHEN** 共享 depth 设为 `High`,agent 发起一轮
- **THEN** 该轮 `ModelRequest.thinking == Some(ThinkingConfig{High})`

#### Scenario: 思考块入 history 并下轮原样回传

- **WHEN** mock provider 返回带 `thinking`(text+signature)的 `ModelResponse`,随后有 tool_call 触发次轮
- **THEN** `Message::Assistant.thinking` 存该块;次轮请求序列化把该块字节一致回传

#### Scenario: 触顶 forced-final 轮也带 depth

- **WHEN** 共享 depth 设为 `High`,mock 触发 `max_iterations` 触顶走 forced-final 收尾请求
- **THEN** 该收尾 `ModelRequest.thinking == Some(ThinkingConfig{High})`(重新读快照,非默认/Off)

#### Scenario: /model 切换剥离旧思考块

- **WHEN** `history` 含带 `signature` 的 `Message::Assistant.thinking`,执行 `/model` 切换
- **THEN** 切换后 history 内所有 `Message::Assistant.thinking` 为空 `Vec`、其余字段不变

#### Scenario: 默认档零回归

- **WHEN** 未调 `set_thinking_depth`(默认 `Low`),既有 agent-loop 测试运行
- **THEN** 仅请求多带一个被 mock 忽略的 thinking 字段,既有断言保持绿、`Agent::new` 签名未变
