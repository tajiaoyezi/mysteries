# config-layering Delta

## ADDED Requirements

### Requirement: 思考默认档位配置

`Config` SHALL 增 `thinking: Depth` 字段,`RawConfig` 增 `thinking: Option<Depth>`(`#[serde(default)]`);`merge` SHALL 以 `project.thinking.or(user.thinking)` 合并(项目优先);`resolve` SHALL 在两级均缺省时填 `DEFAULT_THINKING = Depth::Low`。该字段仅为**会话启动默认档**,运行时 `/think` 覆盖 MUST NOT 回写配置文件。config 层 MUST NOT 做模型能力校验(model 运行时可 `/model` 切换,降级由 wire 层能力表承接)。

#### Scenario: 缺省默认 low

- **WHEN** user 与 project 配置均未设 `thinking`
- **THEN** `resolve()` 后 `Config.thinking == Depth::Low`

#### Scenario: 项目优先合并

- **WHEN** user 设 `thinking="high"`、project 设 `thinking="off"`
- **THEN** merge 后生效 `Depth::Off`(项目覆盖用户)

#### Scenario: 单级设值直接生效

- **WHEN** 仅 user 设 `thinking="medium"`、project 未设
- **THEN** `resolve()` 后 `Config.thinking == Depth::Medium`
