## ADDED Requirements

### Requirement: 运行时 provider 切换

`Agent` SHALL 支持运行时替换其 provider(`set_provider(Arc<dyn Provider>)`),对称于既有「运行时模型切换」。替换后,下一轮编排的模型请求 MUST 经新 provider 发出。为保持切换连贯,`set_provider` 与 `set_model` MUST 将新 provider / model 同步到 Agent 当前的 context strategy(携带 provider 的策略,如 `Compacting`,据此自动压缩走新 provider / model;不携带 provider 的策略,如 `Passthrough`,忽略)。

#### Scenario: 切换后下一轮用新 provider

- **WHEN** 对 `Agent` 调 `set_provider(new)` 后发起一轮 `run`
- **THEN** 该轮的模型调用落在 `new` 上,旧 provider 不被调用

#### Scenario: 切换传播到 context strategy

- **WHEN** Agent 装配了 `Compacting` 策略,随后 `set_provider(new)` / `set_model("m2")`
- **THEN** 此后由该策略触发的自动压缩调用落在 `new` provider / 用模型 `"m2"`(不再用旧 provider / 旧 model)

#### Scenario: Passthrough 策略忽略切换

- **WHEN** Agent 用 `Passthrough` 策略,调 `set_provider(new)`
- **THEN** 策略行为不变(原样返回 history),不报错
