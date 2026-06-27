## ADDED Requirements

### Requirement: 单轮 stdout 对话

系统 SHALL 提供一个二进制入口,接收一条用户 prompt,组装为含 `System` + `User` 的 `ModelRequest`,经 `Provider` 取得一次 `ModelResponse`,并将回复文本输出到 stdout。1.0 仅单轮:取得该次回复后即结束本轮,不进入多轮 Agent Loop。组装与调用逻辑 MUST 落在 IO 无关的 `run_single_turn` 核心函数中,使其可被 `MockProvider` 直接驱动测试;`main` 仅作薄胶水。

#### Scenario: 单轮跑通

- **WHEN** 提供一条 prompt 并执行单轮链路(本 change 由 `MockProvider` 驱动)
- **THEN** 取得的模型回复文本被输出到 stdout,链路正常结束

#### Scenario: 请求组装含 System + User

- **WHEN** 以某 prompt 调用 `run_single_turn`
- **THEN** 传给 provider 的 `ModelRequest.messages` 依次包含一条 `System` 与一条内容为该 prompt 的 `User`

#### Scenario: 流式增量可见

- **WHEN** provider 以多段增量产出回复
- **THEN** 各增量经 `DeltaSink` 按到达顺序输出到 stdout,最终拼成与 `ModelResponse.text` 一致的完整回复
