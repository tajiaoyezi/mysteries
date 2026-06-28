## ADDED Requirements

### Requirement: 活动状态行(输入框上方)

`render` SHALL 在**输入框上方**渲染单行**活动状态行**,承载动态工作状态,由左到右:`spinner`(running 态依 `AppState.spinner_frame` 取帧;`Ready`/`WaitingForPermission` 用静态 glyph)+ phase 文案(见「全 phase 状态行 C10」)+ 运行中(phase 非 `Ready` 且非 `WaitingForPermission`)的 **esc 中断提示** + **token 速率** `↓ N tok · X t/s`(见「token 用量累计与速率呈现」)。活动状态行 MUST **恒占 1 行**(不随 `Ready`/running 改变其行高,避免 transcript 视口高度跳动)。`Ready`/`Idle` 态 MUST **简显**(不显 spinner 动画与 esc 提示)。配色沿用 `设计规范/02` 状态机(running=`accent.primary`、`WaitingForPermission`=`warning.fg`、idle / 速率=`text.muted`)。phase label MUST 出现在活动状态行、MUST NOT 出现在底部状态行。渲染 MUST 可经 `TestBackend` / `insta` 带色快照验证。

#### Scenario: 运行中活动行含 spinner + phase + esc 提示

- **WHEN** `phase = CallingModel`、`spinner_frame` 取某固定帧时渲染到 `TestBackend`
- **THEN** 输入框上方的活动状态行带色快照含 `{spinner} 调用模型…` 与 esc 中断提示(含 `esc`),与锁定一致;底部状态行不含 phase

#### Scenario: Ready 态活动行简显

- **WHEN** `phase = Ready` 时渲染
- **THEN** 活动状态行简显(不含 spinner 动画与 esc 提示),与锁定带色快照一致

#### Scenario: 活动行恒占一行(布局稳定)

- **WHEN** 分别在 `phase = Ready` 与 `phase = CallingModel` 下渲染同尺寸 `TestBackend`
- **THEN** 两态 transcript 视口高度相同(活动状态行高度不变,布局不跳动)

### Requirement: token 用量累计与速率呈现

`tui` 的 `AgentEvent` SHALL 增 `Usage { input_tokens: u32, output_tokens: u32 }` 变体;`ChannelObserver` MUST impl `on_usage` 将每轮 `Usage` forward 为 `AgentEvent::Usage`。`AppState` SHALL 提供纯函数 `record_usage(usage: Usage, elapsed: Duration)`:累计本轮 token 用量(用于 `↓ N tok`,默认取 `output_tokens` 累加)并计算速率(`X t/s` = `output_tokens as f64 / elapsed.as_secs_f64()`;`elapsed` 为 0 时速率为 `None`、MUST NOT 除零 / panic)。`record_usage` MUST NOT 持 `Instant` 或读系统时钟 —— `elapsed` 由调用方传入(守既有 spinner「不把时间引入 `AppState` / `render`」契约),使其可注入合成 `Usage` + `Duration` 单测。`TurnComplete` 与新一轮 `Prompt` MUST 重置本轮累计(与 `iteration` 同语义)。活动状态行 SHALL 显示 `↓ N tok · X t/s`(无可用 usage 时 MUST NOT 显示臆造速率)。**实时流式 t/s 不在本能力范围**:provider `usage` 仅在每次 `complete` 完成后回传、`on_text` 无 token 计数,故速率在**每次 model 调用完成后**刷新,非流式途中逐 token 跳动。

#### Scenario: record_usage 累计 token 并算速率(纯函数)

- **WHEN** 依次 `record_usage(Usage{output_tokens:120,..}, 2s)` 与 `record_usage(Usage{output_tokens:60,..}, 1s)`
- **THEN** 本轮累计 `↓ N tok` 为 `180`,最近速率为 `60.0 t/s`(`60/1`);全程不读系统时钟(elapsed 为入参)

#### Scenario: TurnComplete 重置本轮累计

- **WHEN** `record_usage` 累计若干后 `apply(AgentEvent::TurnComplete)`
- **THEN** 本轮累计 `↓ N tok` 归 0(下一轮重新累计)

#### Scenario: elapsed 为 0 不算速率不 panic

- **WHEN** `record_usage(Usage{output_tokens:50,..}, Duration::ZERO)`
- **THEN** token 累计含 50,速率为 `None`(活动行不显 `t/s`),无除零 / panic

### Requirement: 流式字符估算近似 t/s(标 ~)

流式途中(`CallingModel` 且尚未收到本轮 `Usage`)SHALL 据 `DeltaSink::on_text` 累加的**字符数**与 UI 侧 elapsed(同 Q6,`Instant` 在 IO task)经**纯函数**粗估 token(如 `chars/4`,项目无 tokenizer、**MUST NOT** 伪装为精确值)并计算近似速率,活动行显示 **`~X t/s`**(前缀 `~` 表示估算)。`elapsed` 为 0 时近似速率 MUST 为 `None`、不 panic。收到本轮真实 `AgentEvent::Usage` 并经 `record_usage` 后 MUST **校正**:去掉 `~`,改显真实 `X t/s`(方案 A)。无字符 / 无 elapsed 时 MUST NOT 臆造速率。

#### Scenario: 流式估算标 ~ 且完成后校正

- **WHEN** 流式累计 400 字符、elapsed=2s(尚未收到 usage),随后 `record_usage(Usage{output_tokens:120,..}, 2s)`
- **THEN** 流式阶段活动行含 `~50 t/s`(400/4/2 量级粗估,标 `~`);`record_usage` 后改显真实 `60 t/s`(120/2)、无 `~`

#### Scenario: 流式估算 elapsed 为零不 panic

- **WHEN** 纯函数 `estimate_streaming_rate(100, Duration::ZERO)`
- **THEN** 返回 `None`,无除零 / panic

## MODIFIED Requirements

### Requirement: 全 phase 状态行 C10

**活动状态行**(输入框上方,见「活动状态行(输入框上方)」requirement)SHALL 据 `StatusChanged` 显示完整 phase(`设计规范/02` 状态机):`Idle`→`◇ 就绪`(idle 简显)、`CallingModel`→`调用模型…`、`ExecutingTool(name)`→`执行 {name}…`、`WaitingForPermission`→`▲ 等待授权…`。phase label MUST 渲染在**活动状态行**(输入框上方),MUST NOT 渲染在底部状态行。`AppState` 的 phase 状态 MUST 可单测,渲染 MUST 可 `insta` 快照。

#### Scenario: phase 随事件更新(状态可测)

- **WHEN** `AppState.apply(StatusChanged(ExecutingTool("write_file")))`
- **THEN** 其 phase 为 `ExecutingTool("write_file")`,后续渲染**活动状态行**显示 `执行 write_file…`(底部状态行不含 phase)

#### Scenario: 各 phase 活动状态行快照

- **WHEN** 分别以 `Idle` / `CallingModel` / `ExecutingTool(x)` / `WaitingForPermission` 渲染
- **THEN** 各自 `insta` 快照在**活动状态行**显示对应 glyph + label(正确),底部状态行均不含 phase,与锁定一致

### Requirement: 状态行常驻 meta

**底部状态行** SHALL 常驻显示 `provider · model · iter X/maxIter · N msgs · cwd`(`设计规范/02` C10)。phase label 已移至活动状态行,底部状态行 MUST NOT 再含 phase(原「与左侧 phase 并存」不再适用)。`iter` 由 UI 统计当前轮的 `StatusChanged(CallingModel)` 次数得到(新轮 / `TurnComplete` 重置),`msgs` = `transcript` 中**对话块(`User` / `Assistant`)**数(`Tool` 块与命令产出块 Help / Status / Notice **不计入**);其余取 session 快照(`/model` 切换后 model 同步更新)。

#### Scenario: 底部状态行 meta 快照(不含 phase)

- **WHEN** 给定 session 快照(provider/model/maxIter/cwd)与若干 transcript 块(含 `Tool` 块)渲染
- **THEN** 底部状态行带色快照含 `provider · model · iter X/maxIter · N msgs · cwd`,**不含** phase label,其中 `msgs` 只计 `User` / `Assistant` 块(不含 `Tool`),与锁定一致
