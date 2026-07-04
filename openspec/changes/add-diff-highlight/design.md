# Design — add-diff-highlight

## D1 范围与数据源

diff 体仅 write_file / edit_file(`compute_diff(card.name, card.args)` 非空时);数据源为 **args 纯推导**(write_file → content 全 `Add`;edit_file → old_string 全 `Del` + new_string 全 `Add`),MUST NOT 读文件——沿用 `compute_diff` 既有约定(app.rs 单测 `..._without_reading_files` 锁定)。权限确认框的 diff(warning_bg 风格、不设上限——授权前需完整审阅)不动。

- **被否**:执行后读盘算真实 diff(行级 LCS)——引入 IO 与时序问题(文件可能已再变),且 str-replace 语义下 old→new 块状呈现已忠实;v1 不做。

## D2 展开态渲染

- **插入位置**:头行之后、output 行之前(output 是工具回执文案,保留)。
- **行结构**:`│ `(border_subtle)+ `+ `/`− `/`  ` 标记 + 文本;`Add` → `success.fg`、`Del` → `error.fg`、`Ctx` → `text.body`(`compute_diff` 现不产 `Ctx`,防御性支持);底色 **`bg.base`**(卡片体风格;权限框用 warning_bg,两处风格独立)。
- **超宽**:按既有 `wrap_text` 折行(与 output 一致,保证选区复制完整);续行同色、以两空格占位对齐(不重复 `+ `/`− ` 标记,避免看成多条 diff 行)。
- **上限**:`DIFF_MAX_ROWS = 24`(具名常量,render.rs),按 **DiffLine 逻辑行**计;超出止步并渲尾行 `⋯ 其余 N 行`(`text.muted`,N = 未显示 DiffLine 数)。
  - 取舍:output 无上限是先例,但 write_file 的 content 是整文件、会淹 transcript → 设上限;逻辑行计数简单可测,极端超宽行折行后可超 24 屏行,罕见、记待决。
- **头行 args**:edit/write 展开态改用 `tool_args_preview`(`path=...`)——全量 JSON 与 diff 体内容重复且不可读;preview 缺 path 时既有 fallback(整段 JSON)兜底。其他工具头行不变。

## D3 折叠态摘要

`Done` 且 diff 非空 → ` · +A −D ⌄`(`+A` success.fg、`−D` error.fg,为 0 的一侧省略,如 write_file → ` · +12 ⌄`),替代 ` · N 行 ⌄`;判定位置在既有 exit 分支之后、output 行数分支之前(edit/write 的 exit 恒 None,不冲突)。

- `Running` 维持 `· 运行中…`;`Error` 维持既有摘要——折叠单行显 +/− 会误读为"已应用"。展开态 diff 体则**不分态**渲染(展开是显式动作,呈现"请求的变更"合理,Running/Error 亦可查看)。

## D4 实现形态

- 新函数 `card_diff_lines(card, theme, width) -> Vec<Line<'static>>`:空 diff → 空 Vec(其他工具零侵入);`tool_card_lines` 展开分支插入其结果。
- 行数一致性:transcript 高度/滚动计数与渲染共用 `tool_card_lines` 单一来源,MUST NOT 另写第二套行数计算。
- `diff_line`(权限框版)与其两个调用点(render.rs:153 高度、:936 内容)不改名不改语义。

## D5 快照策略

- **有意 churn 仅一处**:`tui_tool_card_expanded_done`(write_file、args 仅 path → 头行 preview 化;该卡无 content,diff 体为空,其余行不变)。
- **新增快照**(midnight):edit_file 展开 diff 体、write_file 展开全 Add、折叠 `+A −D`、超 `DIFF_MAX_ROWS` 截断。
- 其余既有快照(三态结构、只读徽章、分组、截断、exit、daylight 系列等)**零 churn**,以 `git status --short` 为证。

## 测试断点

- 单测:cap 边界(24 / 25 条)、空 diff 零侵入(非 edit/write 卡与缺参 edit/write 卡行集不变)、折叠计数仅 Done(Running/Error 不显)、`+A −D` 零侧省略。
- 快照:上列 4 新增 + churn 核对。
- 门禁:`cargo test --lib` 全绿、`cargo clippy --all-targets -- -D warnings` 零警告、`openspec validate add-diff-highlight --strict`。
