# config-layering Delta

## MODIFIED Requirements

### Requirement: 上下文压缩配置

运行配置 SHALL 含上下文压缩三项,均可经两层 TOML 分层 merge 覆盖(project 覆盖 user):

- `model_context_window: Option<u32>`(tokens)—— **显式窗口覆盖**:配置后解析链以该值为最高优先;**未配 ≠ 压缩禁用**,有效窗口按「上下文窗口解析」(见 context-strategy)以当前 model 经内置表 / 保守默认求得;
- `compact_trigger_ratio: f32` —— 默认 `0.8`,MUST 落在 `(0.0, 1.0]`,越界 SHALL 报配置错;
- `keep_recent_turns: u32` —— 默认 `1`(压缩时保留的最近完整轮数)。

装配层 SHALL **始终**注入 `Compacting` 策略(压缩默认启用),不再依据 `model_context_window` 是否配置在 `Compacting` / `Passthrough` 间二选一;三项的默认与既有配置项(如 `max_iterations`)一致地由 `resolve` 套用、可被配置覆盖。

#### Scenario: 默认值

- **WHEN** 配置未设压缩三项,`resolve` 得运行配置
- **THEN** `compact_trigger_ratio == 0.8`、`keep_recent_turns == 1`、`model_context_window == None`

#### Scenario: 分层 merge 覆盖

- **WHEN** user 配 `model_context_window = 128000`、project 覆盖 `compact_trigger_ratio = 0.7`
- **THEN** 合并后 `model_context_window == Some(128000)`、`compact_trigger_ratio == 0.7`、`keep_recent_turns` 取默认 `1`

#### Scenario: ratio 越界报错

- **WHEN** 配置 `compact_trigger_ratio = 1.5`(或 `0`)
- **THEN** `resolve` 返回配置错,不静默接受

#### Scenario: window 未配仍启用压缩

- **WHEN** `model_context_window` 未配,装配 agent
- **THEN** 装配层仍注入 `Compacting`(有效窗口按内置表 / 保守默认解析),不再装配 `Passthrough`
