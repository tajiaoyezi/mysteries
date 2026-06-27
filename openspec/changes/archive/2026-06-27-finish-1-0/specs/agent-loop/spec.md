## ADDED Requirements

### Requirement: 运行时模型切换

`Agent` SHALL 提供 `set_model(&mut self, model: String)`,更新后续 `ModelRequest.model` 所用模型。既有 `run` / `run_observed` 的 history / 终止 / 错误 / 事件行为 MUST 不变;`set_model` 只改「下次请求用哪个 model」,不影响进行中的轮。

#### Scenario: set_model 改后续请求的 model

- **WHEN** 对一个 `model = "m1"` 的 `Agent` 调 `set_model("m2")`,再跑一轮(Mock provider)
- **THEN** 该轮 `ModelRequest.model` 为 `"m2"`;其余循环行为与切换前一致(既有 agent-loop 测试保持绿)
