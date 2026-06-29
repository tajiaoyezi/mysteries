## ADDED Requirements

### Requirement: Compacting 运行时 provider / model 切换

`Compacting` SHALL 支持运行时替换其用于摘要的 provider(`set_provider(Arc<dyn Provider>)`)与 model(`set_model(String)`)。替换后,后续 `compact_now` / 自动压缩 MUST 经新 provider / model 发出摘要请求。`ContextStrategy` trait SHALL 暴露增量的默认 no-op `set_provider` / `set_model` 钩子,使非 `Compacting` 策略(如 `Passthrough`)默认忽略切换,`Compacting` override 之以更新自身字段。

#### Scenario: 切换后压缩走新 provider

- **WHEN** 对 `Compacting` 调 `set_provider(new)` + `set_model("m2")`,随后 `compact_now`
- **THEN** 摘要请求落在 `new` provider、用模型 `"m2"`

#### Scenario: Passthrough 默认钩子 no-op

- **WHEN** 对 `Passthrough` 经 `ContextStrategy` 钩子调 `set_provider` / `set_model`
- **THEN** 不报错且行为不变(无 provider 概念,默认实现为空)
