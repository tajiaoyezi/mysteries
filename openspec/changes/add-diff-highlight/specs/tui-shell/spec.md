# tui-shell Delta

## MODIFIED Requirements

### Requirement: 工具卡 C5 渲染

`AppState` SHALL 据 `ToolCallStarted` / `ToolCallFinished` 维护工具卡块;`render` SHALL 按 `设计规范/03` C5 渲染:头(状态 glyph `running`→占位 / `done`→`✓` / `error`→`✗` + 工具名 + args;只读工具带 `只读 · 自动运行` 徽章)、体(`output` 行;截断时 `⋯ +N 行已截断`)、脚(`exit {code}`)。本 change 为**结构态**(最小色,主题留 cut2b;`running` 用静态字符,spinner 留 cut2b)。

**diff 体(write_file / edit_file)**:展开态 SHALL 在头行与 output 行之间渲染着色 diff 体,数据源为 `compute_diff(card.name, card.args)`(args 纯推导,MUST NOT 读文件):`Del` 行 `− ` 前缀 `error.fg`、`Add` 行 `+ ` 前缀 `success.fg`(`Ctx` 防御性 `text.body`),各行带 `│ ` 边框前缀、底色 `bg.base`(权限确认框的 warning_bg diff 风格与上限策略**不变**);超宽按既有 wrap 折行(续行两空格占位对齐、同色,不重复 `+`/`−` 标记);diff **逻辑行数**超 `DIFF_MAX_ROWS`(= 24,具名常量)时 SHALL 止于上限并渲尾行 `⋯ 其余 N 行`(N = 未显示 `DiffLine` 数,`text.muted`)。这两个工具的**展开态头行 args 亦用既有 preview**(`path=...`,内容由 diff 体承载),MUST NOT 再渲整段 JSON。`compute_diff` 为空(参数缺失)或其他工具 MUST 走既有渲染,行集零变化。transcript 行数核算与渲染 MUST 共用同一行集来源(不另设第二套计数)。

**折叠摘要计数**:`Done` 且 diff 非空的 write_file / edit_file,折叠行摘要 SHALL 为 ` · +A −D ⌄`(A / D = `Add` / `Del` 行数,分别 `success.fg` / `error.fg`,**为 0 的一侧省略**),替代 ` · N 行 ⌄`;`Running` / `Error` 折叠摘要 MUST 维持既有形态(不显 +/− 计数,防误读为已应用)。展开态 diff 体不分态渲染(`Running` / `Error` 亦渲,呈现"请求的变更")。

#### Scenario: 工具卡三态结构快照

- **WHEN** 分别以 running / done / error 态的工具卡渲染到 `TestBackend`
- **THEN** `insta` 快照含 C5 结构(glyph + 名 + args + 只读徽章 + output + exit + 截断标记),且与锁定快照一致

#### Scenario: edit_file 展开渲染着色 diff 体

- **WHEN** `tools_expanded`,`Done` 的 edit_file 卡(args 含两行 `old_string`、两行 `new_string`)渲染
- **THEN** 头行 args 为 `path=...` preview(非整段 JSON);头行与 output 行之间依次 2 条 `− `(`error.fg`)+ 2 条 `+ `(`success.fg`)行,各带 `│ ` 前缀、`bg.base` 底;与锁定带色快照一致

#### Scenario: write_file content 全 Add

- **WHEN** `tools_expanded`,write_file 卡(args 含多行 `content`)渲染
- **THEN** diff 体为逐行 `+ `(`success.fg`),行数 = content 行数;与锁定带色快照一致

#### Scenario: 折叠摘要 +A −D 仅 Done 且零侧省略

- **WHEN** 折叠态 `Done` 的 edit_file 卡(old 2 行 / new 3 行)与 write_file 卡(content 12 行);另以同 args 渲染 `Running` / `Error` 态
- **THEN** edit_file 摘要含 ` · +3 −2 ⌄`(`+3` success.fg、`−2` error.fg)、write_file 含 ` · +12 ⌄`(零侧省略);`Running` 仍 `· 运行中…`、`Error` 维持既有摘要,均不显 +/− 计数

#### Scenario: 超 DIFF_MAX_ROWS 截断

- **WHEN** diff 逻辑行数 > 24(如 write_file content 30 行)且 `tools_expanded` 渲染
- **THEN** 恰渲 24 条 diff 行 + 尾行 `⋯ 其余 6 行`(`text.muted`);output 行照常在其后

#### Scenario: 非 diff 工具与空 diff 零回归

- **WHEN** run_shell / read_file 等工具卡,以及 args 缺 `content` / `old_string` / `new_string` 的 write/edit 卡渲染(折叠与展开)
- **THEN** 行集与本 change 前一致;除点名的 `tui_tool_card_expanded_done`(头行 preview 化)外,既有快照零 churn
