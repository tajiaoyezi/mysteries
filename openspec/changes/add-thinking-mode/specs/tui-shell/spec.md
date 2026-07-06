# tui-shell Delta

## ADDED Requirements

### Requirement: 思考过程折叠展示与档位指示

TUI SHALL 通过 `AgentEvent::ThinkingDelta(String)` 承载流式思考文本(`ChannelSink::on_thinking` 发之),`AppState::apply` SHALL 仿 `TextDelta` 累积到 `TranscriptBlock::Thinking(String)`(追加 last_mut 或新建)。`render.rs` 的 `transcript_lines` SHALL 为 `TranscriptBlock::Thinking` **默认展开**渲染:正文上方 SHALL 有一行 header `✻ 思考`(`text_secondary`+`BOLD`,与 `text_muted` 灰字 body 区分);body 为思考正文(每行 `  ` 缩进)。**仅折叠溢出部分**:设 body 渲染后行数为 L、阈值 `THINKING_FOLD_THRESHOLD = 12`(header **不计入** L)——`L ≤ 12` 时 body **全显**、不出折叠标记;`L > 12` 时显示 header + **前 12 行 body** + 一行折叠标记 `… +{M} 行(Ctrl+O 展开)`(`M = L − 12`,`text_muted`);`tools_expanded`(Ctrl+O)为真时 header + body **全显**。MUST NOT 新增独立键位(复用 `tools_expanded`)。footer/状态区 SHALL 显示当前思考档位(仿权限模式指示器)。当模型思考无法关闭(能力 `can_disable:false`)而档位为 `Off` 时,SHALL 出一行提示"该模型思考无法关闭"。TUI 展示走 `TestBackend`+`insta` 事后快照,不走 red-green。

#### Scenario: 思考流式累积成块

- **WHEN** 连续 `ThinkingDelta("思考")` 与 `ThinkingDelta("片段")`
- **THEN** 归入同一 `TranscriptBlock::Thinking`,文本为 `思考片段`

#### Scenario: 短思考默认全显(不折叠)

- **WHEN** Thinking 块 body 渲染后 ≤ 12 行、`tools_expanded=false`
- **THEN** 显示 header `✻ 思考` + 全部灰字正文,无折叠标记

#### Scenario: 长思考默认只折叠溢出尾部

- **WHEN** Thinking 块 body 渲染后为 20 行、`tools_expanded=false`
- **THEN** 显示 header + 前 12 行灰字正文 + 一行 `… +8 行(Ctrl+O 展开)` 折叠标记;快照锁定

#### Scenario: Ctrl+O 展开长思考

- **WHEN** 同一 20 行 body、`tools_expanded=true`
- **THEN** header + 20 行灰字正文全显、无折叠标记;快照锁定

#### Scenario: footer 显示当前档位

- **WHEN** 当前思考档为 `High`
- **THEN** footer/状态区显示当前档位(暗/亮主题各锁快照)

#### Scenario: 恒开模型 Off 提示

- **WHEN** 当前模型 `can_disable:false` 且档位 `Off`
- **THEN** 出一行"该模型思考无法关闭"提示
