# tui-shell Delta

## MODIFIED Requirements

### Requirement: 工具卡 C5 渲染

`AppState` SHALL 据 `ToolCallStarted` / `ToolCallFinished` 维护工具卡块;`render` SHALL 按 `设计规范/03` C5 渲染:头(状态 glyph `running`→spinner 动画帧 / `done`→`✓` / `error`→`✗` + 工具名 + args;只读工具带 `只读 · 自动运行` 徽章)、体(`output` 行;截断时 `⋯ +N 行已截断`)、脚(`exit {code}`)。着色契约见「themed 渲染」requirement,running 态 spinner 见「spinner 动画(确定性渲染)」requirement。

**diff 体(write_file / edit_file)**:展开态 SHALL 在头行与 output 行之间渲染着色 diff 体,数据源为 `compute_diff(card.name, card.args)`(args 纯推导,MUST NOT 读文件):`Del` 行 `− ` 前缀 `error.fg`、`Add` 行 `+ ` 前缀 `success.fg`,各行带 `│ ` 边框前缀(`border_subtle`)、底色 `bg.base`(权限确认框的 warning_bg diff 风格与不设上限策略**不变**)。(实现 MAY 防御性处理 `Ctx` → `text.body`;`compute_diff` 现不产 `Ctx`,不在契约面。)

**宽度与折行**:diff 行内容宽 MUST 为 `width.saturating_sub(4).max(1)`(整行宽 − `│ ` 2 列 − 标记 2 列;区别于 output 行的 −2),按既有 wrap 以显示宽度折行;续行以 `│ ` + 两空格占位起头(恰补标记列,首行续行内容同宽)、同色、MUST NOT 重复 `+ `/`− ` 标记;任一屏行显示宽 MUST NOT 超视口宽(不得溢出被截,保选区复制完整)。

**截断(按屏行)**:diff 体 SHALL 以 `DIFF_MAX_ROWS`(= 24,具名常量)为**屏行预算**(按折行后的显示行计,允许止于某条 `DiffLine` 的折行中途);超预算时止于预算并渲尾行 `⋯ 其余 N 行`(N = 未被**完整**显示的 `DiffLine` 数,`text.muted`,带 `│ ` 边框前缀)。

**头行 args**:这两个工具的展开态头行 args 亦 SHALL 用既有 `tool_args_preview`(`path=...`;preview 缺 `path` 时沿用既有整段 JSON fallback),除该 fallback 外 MUST NOT 渲整段 JSON。展开态 diff 体与头行 preview SHALL **不分态**渲染(`Running` / `Error` 亦同,呈现"请求的变更")。

**空 diff 与其他工具**:`compute_diff` 为空(diff 参数缺失或为空串——write 的 `content`;edit 的 `old_string` 与 `new_string` **均**缺失/空串)或非 write/edit 工具时,MUST 不渲染任何 diff 体行:非 write/edit 工具行集与既有完全一致;空 diff 的 write/edit 卡折叠态与既有一致、展开态除头行 args 改 preview 外其余行一致。transcript 行数核算与渲染 MUST 共用同一行集来源(不另设第二套计数)。

**折叠摘要计数**:`Done` 且 diff 非空的 write_file / edit_file,折叠行结果摘要 SHALL 为 ` · +A −D ⌄`(A / D = `Add` / `Del` 行数,`+A` 用 `success.fg`、`−D` 用 `error.fg`,`−` 与 diff 前缀同字符 U+2212,点缀 ` · ` 与 ` ⌄` 用 `text.secondary`,**为 0 的一侧省略**);判定优先级与属主契约见 MODIFIED「工具输出折叠与全局展开(ctrl+o)」。`Running` / `Error` 折叠摘要 MUST 维持既有形态(不显 +/− 计数,防误读为已应用)。

#### Scenario: 工具卡三态结构快照

- **WHEN** 分别以 running / done / error 态的工具卡渲染到 `TestBackend`
- **THEN** `insta` 快照含 C5 结构(glyph + 名 + args + 只读徽章 + output + exit + 截断标记),且与锁定快照一致

#### Scenario: edit_file 展开渲染着色 diff 体

- **WHEN** `tools_expanded`,`Done` 的 edit_file 卡(args 含 `path`、两行 `old_string`、两行 `new_string`,output 非空)渲染
- **THEN** 头行 args 为 `path=...` preview(非整段 JSON);头行与 output 行之间依次 2 条 `− `(`error.fg`)+ 2 条 `+ `(`success.fg`)行,各带 `│ ` 前缀(`border_subtle`)、`bg.base` 底;与锁定带色快照一致

#### Scenario: write_file content 全 Add

- **WHEN** `tools_expanded`,`Done` 的 write_file 卡(args 含 `path` 与多行 `content`,output 非空)渲染
- **THEN** diff 体为逐行 `+ `(`success.fg`),条数 = content 行数;与锁定带色快照一致

#### Scenario: 折叠摘要 +A −D 仅 Done、零侧双向省略

- **WHEN** 折叠态 `Done` 的 edit_file 卡(old 2 行 / new 3 行)、write_file 卡(content 12 行)、edit_file 卡(old 2 行 / `new_string` 空串);另以同 args 渲染 `Running` / `Error`(output 2 行、无 exit)态
- **THEN** 三张 Done 卡摘要分别含 ` · +3 −2 ⌄`(`+3` success.fg、`−2` error.fg)、` · +12 ⌄`(Del 侧省略)、` · −2 ⌄`(Add 侧省略);`Running` 仍 ` · 运行中…`、`Error` 仍 ` · 2 行 ⌄`,均不显 +/− 计数

#### Scenario: 超 DIFF_MAX_ROWS 截断(短行)

- **WHEN** `tools_expanded`,write_file 卡 content 为 30 条短行(各占 1 屏行)渲染
- **THEN** 恰渲 24 条 diff 屏行 + 尾行 `⋯ 其余 6 行`(`text.muted`,带 `│ ` 前缀);output 行照常在其后;transcript 行数核算含 diff 行与尾行

#### Scenario: 单条超长行按屏行截断(minified 场景)

- **WHEN** `tools_expanded`,write_file 卡 content 为**单条**超长行(如显示宽 1200,窄视口下折行后 > 24 屏行)渲染
- **THEN** diff 体恰 24 条屏行(止于该逻辑行折行中途)+ 尾行 `⋯ 其余 1 行`(该 `DiffLine` 未被完整显示,计 1)

#### Scenario: 超宽 diff 行折行(窄视口、含 CJK)

- **WHEN** `tools_expanded`,write_file 卡 content 含一条显示宽度超视口的长行(含 CJK 宽字符),渲染到窄 `TestBackend`(如 width = 40)
- **THEN** 该 `DiffLine` 折为多条屏行:首行以 `│ ` + `+ ` 起,续行以 `│ ` + 两空格占位起、同 `success.fg`、不重复 `+ `;各屏行显示宽 ≤ 视口宽(内容宽 = 整行宽 − 4);与锁定带色快照一致

#### Scenario: Running / Error 展开不分态渲染 diff 体

- **WHEN** `tools_expanded`,同一 edit_file args(含 `path`、old 2 行、new 2 行)分别以 `Running` / `Error`(output 非空)态渲染
- **THEN** 两态头行 args 均为 `path=...` preview;头行后均渲 2 条 `− ` + 2 条 `+ `(着色同 Done);`Running` 的体行为「运行中…」占位、`Error` 的体行为 output 文本;与锁定带色快照一致

#### Scenario: 空 diff 与非 diff 工具零回归

- **WHEN** run_shell / read_file 等非 write/edit 工具卡,以及**缺齐 diff 参数**(即 `compute_diff` 为空:write 缺 `content` 或 `content` 为空串;edit 的 `old_string`/`new_string` 均缺失)的 write/edit 卡,分别以折叠与展开渲染
- **THEN** 均不渲任何 diff 体行、不显 +/− 计数;非 write/edit 工具卡行集与既有锁定快照一致;空 diff 的 write/edit 卡折叠态与既有一致、展开态仅头行 args 为 `path=...` preview(既有快照仅 `tui_tool_card_expanded_done` 因此更新);其余既有快照与锁定一致

### Requirement: 工具输出折叠与全局展开(ctrl+o)

`AppState` SHALL 持折叠态 `tools_expanded: bool`(默认 `false` = 折叠)。`render` MUST 据 `tools_expanded` 对每个 `TranscriptBlock::Tool(ToolCard)` 块二选一渲染:**折叠**态仅渲染 `设计规范/03` C5 的**单行头**(状态 glyph + 工具名 + args 摘要 + 结果摘要),**不**渲染 output 体与脚,且单行头 **MUST NOT 含** `┌─` 边框前缀(`┌─` / `└─` 边框仅展开态用);**展开**态渲染**全量**(头 / diff 体(仅 write_file / edit_file 且 diff 非空,见「工具卡 C5 渲染」)/ 体 / 脚 + 截断标记)。折叠行结果摘要 SHALL 按序判定:`running` → ` · 运行中…`;携 `exit` → ` · exit {code}`(非 0 用 `error.fg`);`done` 且为 write_file / edit_file 且 diff 非空 → ` · +A −D ⌄`(定义与着色见「工具卡 C5 渲染」);其余 output 非空 → ` · {N} 行 ⌄`(N = output 行数,`⌄` 提示可展开)。

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
