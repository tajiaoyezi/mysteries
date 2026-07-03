## Context

现渲染(`src/tui/render.rs`):`transcript_lines`(:339)逐 `TranscriptBlock` 产 `Vec<Line<'static>>`;`Assistant(text)` 臂(:352)走 `message_lines("◆ ", text, info_fg marker, text_body 正文)`——`text.split('\n')` 后每物理行 `wrap_text`(:432,纯字符宽换行)成单一 `text_body` 样式的 `Line`。高度经 `transcript_line_count`(:57)按同一渲染算行数。`Assistant` 文本流式累积(app.rs:870 `current.push_str`),每帧重渲。

`Theme`(theme.rs:4-25):20 个语义色字段(逐字段见 `src/tui/theme.rs:4-25`,含 `error_border`),**无明暗判别字段**。`midnight()`/`daylight()` 两构造。

## Goals / Non-Goals

**Goals:**
- `Assistant` 输出按 markdown(CommonMark+GFM)渲染:标题/强调/行内码/围栏码(语法高亮)/列表/引用/hr/链接/简易表格。
- 仅作用于 `Assistant` 块;`◆ ` marker + 续行缩进保留;纯文本(无 md 语法)输出尽量贴近现状以控快照 churn。
- 流式安全:半截 markdown 不崩、优雅渲染。

**Non-Goals(v1):**
- 代码块行号、链接可点击/OSC8、表格单元格内换行与列合并、深层嵌套引用、数学公式。
- 不改 `visual_input_layout`(输入框布局);不改其它 `TranscriptBlock` 渲染。
- markdown token→我方调色板的**精细 scope 映射**(代码块直接用 syntect 内置主题色)。

## Decisions

- **D1 新模块 `render_markdown(text, theme, width) -> Vec<Line<'static>>`(`src/tui/markdown.rs`)。** 用 `pulldown_cmark::Parser::new_ext(text, Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH)` 走事件流(`Event::Start(Tag)`/`End(TagEnd)` 拆分形态,pulldown-cmark 0.13),维护"当前 inline 样式栈 + 当前块类型"状态机,产出带样式 span 的逻辑行,再经 D4 span 换行到 `width`。**`Event::SoftBreak` 与 `Event::HardBreak` 均映射为硬换行**(结束当前逻辑行、起新行),**不**按 CommonMark 默认把 SoftBreak 折成空格——以贴合现有 `message_lines` 的 `text.split('\n')` 逐物理行行为,保纯文本 assistant 行结构不变(修 review major finding)。相邻块级元素间插一条空逻辑行分隔(计入 `render_markdown` 行数,`transcript_line_count` 随之自洽;区别于主 spec 的 `TranscriptBlock` 整块间空行)。所有 span 内容 MUST 为 owned(`CowStr::into_string`/`into_static` / `&str.to_string()`)以产 `Span<'static>`。`transcript_lines` 的 `Assistant` 臂改为:`◆ ` 首行 marker + 后续行缩进,正文用 `render_markdown` 的行(marker/缩进由 Assistant 臂包,markdown 只管正文 width = 总宽 − marker 宽)。

- **D2 元素 → 样式映射(`Theme` 语义色 + `Modifier`)。**
  - 标题 H1..H6:`text_title` + `BOLD`(H1 additionally `accent_primary`);段落:`text_body`。
  - `**strong**`:`+BOLD`;`*em*`:`+ITALIC`;`***both***`:两者;`~~del~~`:`+CROSSED_OUT` + `text_muted`。
  - `inline code`:`text_body` on `bg_surface_alt`。
  - 列表:无序 `• `/有序 `N. ` marker(`text_secondary`),按嵌套层缩进 2 空格;`> quote`:每行 `▎ ` 前缀(`border_strong`)+ `text_secondary`;`---` hr:整行 `─`×width(`border_subtle`)。
  - 链接 `[t](u)`:文字 `accent_primary` + `UNDERLINED`;URL 不内联(v1;可 dim 尾注留后续)。
  - 图片/HTML 原样文本(不解析渲染)。

- **D3 代码块语法高亮:`syntect`(default-fancy / regex-fancy 引擎,懒加载)。** `Cargo.toml`:`syntect = { version = "5", default-features = false, features = ["default-fancy"] }`(纯 Rust、无 onig C 依赖;含默认 syntax/theme dump)。`std::sync::LazyLock`(Rust 1.80+ 稳定,**无需第三方**——本仓未钉 MSRV、once_cell 现仅传递依赖)全局持 `SyntaxSet::load_defaults_newlines` + `ThemeSet::load_defaults`,首访加载一次。围栏码块:取 info string 首词为语言→`syntax_set.find_syntax_by_token(lang).unwrap_or_else(|| plain_text)`;按 `theme.is_dark` 选 syntect 主题(暗:`base16-ocean.dark`;亮:`InspiredGitHub` 或 `Solarized (light)`,取存在者);`HighlightLines` 逐行高亮→`(syntect::Style, &str)`→`syntect_color_to_ratatui(Color)` 转 span;块整体加 `bg_sunken` 底 + 首行语言标签(`text_muted`)。**语言未知**退化为 plain(仅底色、不高亮)。

- **D4 span 感知换行(新 helper,不碰 `visual_input_layout`)。** `wrap_spans(spans: &[Span<'static>], width) -> Vec<Vec<Span<'static>>>`(**直接产 `Span` 便于 `Line::from`**;render_markdown 内部中间态亦用 `Span`):按 `width::char_width` 累计显示宽,达 width 换行,跨行保留各段样式(字符级换行,与既有 `wrap_text` 口径一致;CJK 宽字符按 2 计)。**截断口径(代码块行 / 表格单元格)统一按 `char_width` 显示宽**:预留 `…` 的显示宽、逐字符累加,当再加下一字符会越界则整字符舍去(**绝不半切宽字符**,留下的 1 列空缺由填充补),保证 截断串 + `…` 的显示宽 ≤ 行宽/列宽;表格填充与截断同用 `display_width`(width.rs 已有)使含 CJK 列严格对齐。代码块行**不换行**(超宽即按上述截断,v1;真机看)。

- **D5 `Theme` 加 `is_dark: bool`。** `midnight()` 设 `true`、`daylight()` 设 `false`。供 D3 选 syntect 主题;个别元素(如 inline code 底色深浅)亦可据此微调。仅加字段,既有 `Theme` 构造两处补一行,`PartialEq`/`Clone` 派生不受影响。

- **D6 流式 / 半截 markdown。** 每帧对整个 `Assistant` 文本重解析重渲(pulldown-cmark 对半截输入尽力而为:未闭合围栏→按代码块尽量渲、未闭合强调→按普通文本)。**性能**:重高亮成本集中在 syntect;v1 先每帧重渲(单条消息小,syntect 逐行高亮亚毫秒级),**若真机长代码块流式卡顿**,再加"按 `(text,width,is_dark)` 缓存已完成块渲染、只重渲在流的块"(mitigation 留 D6+,不入 v1 除非真卡)。

- **D7 作用域与复制。** 仅 `Assistant` 块;`User`/`Notice`/`Help`/`Error`/`Tool`/`Status` 不动。选区复制取渲染后文本(markdown 标记隐藏后所见即所得),接受。纯文本 assistant(无 md 语法)经 pulldown-cmark 解析为段落→`text_body` 正文,**力求与现状逐字节接近**但不保证完全一致,含 assistant 的既有快照按预期 churn 处理(review diff 后重接受)。

## Alternatives considered

- **手写 markdown parser**——全量 CommonMark+GFM 边界巨多、易错,库更正确;弃。
- **不引 syntect、代码块只区分底色**——省最大依赖,但 agent 代码输出高价值;用户已选 v1 做高亮。弃(留作降级备选)。
- **syntect scope→我方 20 色精细映射**——工作量大、观感未必更好;v1 直接用 syntect 内置暗/亮主题色。弃。
- **syntect 用默认 `onig`(C)**——引 C 依赖、跨平台构建风险;改 `default-fancy`(regex-fancy 纯 Rust 引擎)。弃。
- **改 `visual_input_layout` 复用其换行**——那是输入框布局、耦合光标/滚动;markdown 换行诉求不同,另写 `wrap_spans`。弃。

## Risks / Trade-offs

- **syntect 体积/加载**:默认 syntax/theme dump 使二进制增大几 MB(项目最大依赖);懒加载摊到首个代码块渲染。接受。
- **流式重高亮成本**:见 D6,v1 每帧重渲;真机若卡再上缓存。
- **span 换行宽字符边界**:CJK/emoji 宽度按 `char_width`,与既有口径一致;宽字符跨 width 边界的断行需测。
- **syntect 主题 vs 双调色板观感**:代码块用 syntect 自己的暗/亮主题色,可能与 Midnight/Daylight 整体不完全协调;v1 接受,真机看是否要换主题或做 scope 映射。
- **assistant 快照 churn**:含 assistant 文本的既有快照大概率变;review diff 确认非渲染回归后重接受。
- **表格宽度**:简易表格按内容列宽,超总宽时如何缩/截断需定(v1:按可用宽等比或截断,真机调)。
