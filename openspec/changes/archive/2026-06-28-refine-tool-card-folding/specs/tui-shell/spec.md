## MODIFIED Requirements

### Requirement: 工具输出折叠与全局展开(ctrl+o)

`AppState` SHALL 持折叠态 `tools_expanded: bool`(默认 `false` = 折叠)。`render` MUST 据 `tools_expanded` 对每个 `TranscriptBlock::Tool(ToolCard)` 块二选一渲染:**折叠**态仅渲染 `设计规范/03` C5 的**单行头**(状态 glyph + 工具名 + args 摘要 + 结果摘要),**不**渲染 output 体与脚,且单行头 **MUST NOT 含** `┌─` 边框前缀(`┌─` / `└─` 边框仅展开态用);**展开**态渲染**全量**(头 / 体 / 脚 + 截断标记,即现状)。折叠行结果摘要 SHALL 为:`done` 且 output 非空 → ` · {N} 行 ⌄`(N = output 行数,`⌄` 提示可展开);携 `exit` → ` · exit {code}`(非 0 用 `error.fg`);`running` → ` · 运行中…`。

连续的 `Tool` 块 SHALL 视为一组:**组内相邻 `Tool` 块之间 MUST NOT 插入空行**(紧凑呈现);组边界(相邻块非 `Tool`,或位于 transcript 端点)仍插入空行分隔。折叠态下,每个连续 `Tool` 组的**组首**卡片 SHALL 在结果摘要后追加 ` · ctrl+o 展开`(`text.muted`);**组内非组首**卡片与**展开**态 MUST NOT 追加该提示(每组仅一次、补展开可发现性)。

`ctrl+o`(`KeyCode::Char('o')` + `KeyModifiers::CONTROL`,**仅** `KeyEventKind::Press`)MUST 翻转 `tools_expanded`(全局展开/折叠**所有**工具卡),且 MUST NOT 把 `o` 写入输入框(在文本输入 arm 之前拦截)。折叠**仅作用于** `Tool` 块;`User` / `Assistant` / `Error` / `Help` / `Status` / `Notice` 块 MUST NOT 受折叠影响。本期**只**提供全局 toggle,不提供单条卡片独立展开。

#### Scenario: 工具卡默认折叠为单行(带色快照)

- **WHEN** 一个 `done` 且 output 多行的 `Tool` 块(transcript 中唯一,即所在组组首)在 `tools_expanded == false`(默认)下渲染到 `TestBackend`
- **THEN** 带色快照仅含该卡的单行头(glyph + 工具名 + args + ` · {N} 行 ⌄` + ` · ctrl+o 展开`),**不**含 `┌─` 头边框、output 体行与 `└─` 脚,与锁定一致

#### Scenario: ctrl+o 全局展开再折回(逻辑 + 快照)

- **WHEN** 对含若干 `Tool` 块的 `AppState` 投入 `ctrl+o`(`Char('o')`+`CONTROL`,`Press`)一次,再投一次
- **THEN** 第一次后 `tools_expanded == true` 且所有 `Tool` 块渲为全量(头 + 体 + 脚 + 截断标记);第二次后 `tools_expanded == false` 折回单行;两态带色快照各与锁定一致

#### Scenario: ctrl+o 仅 Press 且不写入输入框

- **WHEN** 依次投入 `ctrl+o` 的 `Release` 与 `Repeat`,再投入 `Press`
- **THEN** 仅 `Press` 翻转 `tools_expanded`;`Release` / `Repeat` 不翻转;任一情况下输入框串均不出现字符 `o`

#### Scenario: 折叠仅作用于 Tool 块

- **WHEN** transcript 含 `User` / `Assistant` / `Tool`(done)三块且 `tools_expanded == false` 时渲染
- **THEN** `User` / `Assistant` 块仍**全文**渲染(不折叠),仅 `Tool` 块折为单行,与锁定带色快照一致

#### Scenario: 连续工具卡分组紧凑且仅组首带展开提示(带色快照)

- **WHEN** transcript 为 `User` → `Tool`(read,done)→ `Assistant` → `Tool`(write,done)→ `Tool`(grep,done),在 `tools_expanded == false` 下渲染
- **THEN** read 与 write 各为所在组组首、行尾带 ` · ctrl+o 展开`;grep 为 write 组的组内非首、**不**带该提示;write 与 grep 之间**无**空行(同组紧凑);与锁定带色快照 `tui_tool_group_ctrl_o_hints` 一致
