## ADDED Requirements

### Requirement: Assistant 消息 markdown 渲染(CommonMark+GFM,代码块语法高亮)

TUI SHALL 把 `TranscriptBlock::Assistant(text)` 的正文按 markdown(CommonMark + GFM 表格/删除线)渲染为带样式的 `ratatui::Line`,经 `src/tui/markdown.rs` 的 `render_markdown(text, theme, width) -> Vec<Line<'static>>`;其余 `TranscriptBlock` 变体 MUST NOT 受影响。渲染 MUST 用 `pulldown-cmark` 解析、`syntect`(`default-fancy` feature、纯 Rust regex-fancy 引擎、不引 C `onig`)做围栏代码块语法高亮。

**元素映射**:标题 H1..H6 → `text_title`+`BOLD`(H1 加 `accent_primary`);`**strong**` → `+BOLD`、`*em*` → `+ITALIC`、`~~del~~` → `+CROSSED_OUT`+`text_muted`;`inline code` → `text_body` on `bg_surface_alt`;无序/有序列表 → `• `/`N. ` marker + 按嵌套层缩进;`> quote` → `▎ ` 前缀 + `text_secondary`;`---` → 整行 `─`;链接 `[t](u)` → 文字 `accent_primary`+`UNDERLINED`(URL v1 不内联)。

**代码块**:围栏码块按 info string 首词取语言,`syntect` 逐 token 上色(未知语言退化为纯色),块加区分底色(`bg_sunken`)+ 语言标签;syntect 的 `SyntaxSet`/`ThemeSet` MUST 经 `std::sync::LazyLock`(标准库,不新增第三方)懒加载一次;syntect 主题按 `Theme.is_dark` 选暗/亮;syntect `Color{r,g,b}` 转 ratatui `Color`;`HighlightLines::highlight_line` 返回 `Result` MUST 优雅处理(不 `unwrap`,退 plain)以保半截 markdown 不 panic。

**Theme**:`Theme` SHALL 增 `is_dark: bool` 字段(`midnight()`=`true`、`daylight()`=`false`),供 syntect 选主题。

**行结构**:段落内 `Event::SoftBreak` 与 `Event::HardBreak` MUST 均映射为硬换行(起新逻辑行),**不**按 CommonMark 默认把 SoftBreak 折成空格——以贴合现有 `text.split('\n')` 逐物理行、保纯文本 assistant 行结构不变。相邻块级元素(标题/段落/列表/引用/代码块/表格/hr)之间 SHALL 插入一条空逻辑行分隔,该空行计入 `render_markdown` 产出行数(故 `transcript_line_count`/高度核算随之自洽);此为 **Assistant 单条消息内 markdown 块间**分隔,区别于主 spec 的 `TranscriptBlock` 整块间空行。**注**:CommonMark 会把连续多个空行折成单一段落边界、并 strip 首尾空行,故含多空行/首尾空行的纯文本 assistant 行数可能与 `split('\n')` 现状不同(属预期 churn,不在"逐字节一致"承诺内——该承诺仅限非-assistant 块)。

**换行/截断**:markdown 行内内容 MUST 经 span 感知换行 `wrap_spans(spans: &[Span<'static>], width) -> Vec<Vec<Span<'static>>>` 按 `width` 断行且跨行保留各 span 样式,宽度用 `width::char_width`(CJK 计 2);MUST NOT 改动 `visual_input_layout`(输入框布局)。代码块行与表格单元格**超宽截断 MUST 按 `char_width` 显示宽计**(预留 `…` 显示宽、逐字符累加、**不半切宽字符**),表格填充与截断同用 `display_width` 使含 CJK 列严格对齐。`transcript_lines` 的 `Assistant` 臂 MUST 保留 `◆ ` 首行 marker + 续行缩进,markdown 正文宽度 = 总宽 − marker 宽;`transcript_line_count`/高度核算 MUST 与实际渲染同源(复用 `render_markdown`),不得行数与渲染不一致。

**流式**:`Assistant` 文本流式累积、每帧重解析重渲;半截 markdown(未闭合围栏/强调)MUST 优雅渲染不 panic。选区复制取渲染后文本(markdown 标记隐藏)。

#### Scenario: 行内强调与行内码渲染为带样式 span

- **WHEN** `Assistant` 正文为 `普通 **粗** *斜* 与 `代码``,`render_markdown` 渲染
- **THEN** `粗` 段 span 含 `BOLD`、`斜` 段含 `ITALIC`、`代码` 段 fg=`text_body` 且 bg=`bg_surface_alt`、`普通` 段为 `text_body` 无修饰;标记 `**`/`*`/`` ` `` 不出现在渲染文本中

#### Scenario: 围栏代码块按语言语法高亮

- **WHEN** `Assistant` 正文含 ` ```rust\nfn main() {}\n``` `
- **THEN** 代码块整体加区分底色(`bg_sunken`)、带 `rust` 语言标签;`fn`(关键字)与 `main`(标识符)渲成**不同 fg**(syntect 高亮);`SyntaxSet`/`ThemeSet` 仅懒加载一次

#### Scenario: 未知语言代码块退化为纯色块

- **WHEN** 围栏 info 为未知语言(如 ` ```zzz `)或无语言
- **THEN** 代码块仍加区分底色但不逐 token 上色(纯 `text_body`),不 panic

#### Scenario: 暗/亮主题选不同 syntect 主题

- **WHEN** 同一代码块分别在 `Theme::midnight()`(is_dark=true)与 `daylight()`(is_dark=false)下渲染
- **THEN** 分别选用暗/亮 syntect 主题,代码 token 颜色随之不同;两快照各自锁定

#### Scenario: 列表/引用/标题/hr 渲染

- **WHEN** `Assistant` 正文含 `# 标题`、`- a\n  - b`(嵌套)、`> 引用`、`---`
- **THEN** 标题 `text_title`+BOLD;`a` 一级 `• ` marker、`b` 二级缩进 `• `;引用行 `▎ ` 前缀 + `text_secondary`;hr 为整行 `─`;**且相邻块级元素之间各有一条空逻辑行分隔(计入总行数)**

#### Scenario: 简易 GFM 表格按列宽对齐

- **WHEN** `Assistant` 正文含一个 2 列 GFM 表(含 CJK 单元格)
- **THEN** 表头(`text_title`+BOLD)+ 分隔行(`─`)+ 数据行按各列最大 `display_width` 左对齐;单元格超列宽按 `display_width` 截断 + `…`(预留宽、**不半切宽字符**,填充与截断同用显示宽 → 含 CJK 列各行列位严格对齐);不做单元格内换行

#### Scenario: 仅 Assistant 受影响、其余块不变

- **WHEN** transcript 含 `User`/`Notice`/`Help`/`Error`/`Tool` 各块
- **THEN** 这些块渲染与引入 markdown 前**逐字节一致**(既有非-assistant 快照零 churn);仅 `Assistant` 块走 markdown

#### Scenario: 半截 markdown 流式不 panic

- **WHEN** 流式中途 `Assistant` 文本为未闭合围栏(`` ```rust\nfn ``,无收尾 ` ``` `)
- **THEN** `render_markdown` 尽力渲染(按代码块渲已到达部分)、不 panic;闭合后正常高亮

#### Scenario: 纯文本多行 assistant 每个换行成独立行(SoftBreak 不折空格)

- **WHEN** `Assistant` 纯文本正文为段落内含单 `\n` 的两行(如 `alpha\n你好 beta`,无 markdown 语法)
- **THEN** 每个 `\n`(SoftBreak)仍渲成**独立逻辑行**(续行照 `◆ ` 缩进),**不**被合并为一段加空格;行结构与引入 markdown 前的 `text.split('\n')` 一致
