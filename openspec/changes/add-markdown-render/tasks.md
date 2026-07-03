> 注:markdown 渲染属 **TUI 外壳**(产 ratatui `Line`/`Span`),按 CLAUDE.md 走**事后**回归(insta 快照 + 针对性 span 单测),不走 red-green。每节实现后即验。

## 1. 依赖 + Theme.is_dark

- [ ] 1.1 `Cargo.toml` 加 `pulldown-cmark`(默认或启 GFM 所需 feature)、`syntect = { version = "5", default-features = false, features = ["default-fancy"] }`(纯 Rust regex-fancy 引擎 + 默认 syntax/theme dump)。**懒加载用 `std::sync::LazyLock`(标准库,不引 once_cell、不新增第三方)**。仅 2 个新增依赖(pulldown-cmark、syntect),说明理由(见 design)。`cargo build` 通过。
- [ ] 1.2 `src/tui/theme.rs`:`Theme` 加 `is_dark: bool`;`midnight()`=`true`、`daylight()`=`false`。既有测试/构造随之补;`cargo test --lib theme` 绿。

## 2. markdown 核心:inline + 段落 + span 换行

- [ ] 2.1 `src/tui/markdown.rs`:`render_markdown(text, theme, width) -> Vec<Line<'static>>` 骨架 + pulldown-cmark 事件流状态机(inline 样式栈)。先落**段落 + 行内**:`**strong**`(BOLD)、`*em*`(ITALIC)、`***both***`、`~~del~~`(CROSSED_OUT+muted)、`inline code`(bg_surface_alt)、链接(accent+UNDERLINED)。**`Event::SoftBreak` 与 `Event::HardBreak` 均映射为硬换行(结束当前逻辑行、起新行),不折成空格——贴合现有 `text.split('\n')` 逐物理行,保纯文本 assistant 行结构不变。** **所有权**:pulldown `Event::Text(CowStr)`/syntect `highlight_line` 返回 `&str` 均借自输入 `&text`;产 `Span<'static>` MUST 转 owned(`CowStr::into_string`/`Event::into_static` / `&str.to_string()`),比照 render.rs 现有 `Span::styled(text.to_string(), style)`。
- [ ] 2.2 span 感知换行 `wrap_spans(spans: &[Span<'static>], width) -> Vec<Vec<Span<'static>>>`(直接产 `Span` 便于 `Line::from`;按 `width::char_width`,字符级,跨行保样式,CJK 宽 2)。接进 render_markdown 输出。
- [ ] 2.3 单测:一段含 `**b** *i* `code` [t](u)` 的文本 → 断言产出 span 的 `Modifier`/`fg`/`bg` 对位;`wrap_spans` 对含 CJK 的样式串在窄宽下断行正确、样式不丢。

## 3. 块元素:标题 / 列表 / 引用 / hr

- [ ] 3.1 标题 H1..H6(text_title+BOLD,H1 加 accent);`---` hr(整行 `─`);段间空行。
- [ ] 3.2 列表:无序 `• `/有序 `N. `(marker text_secondary),嵌套按层缩进 2 空格;`> quote` 每行 `▎ ` 前缀(border_strong)+ text_secondary。
- [ ] 3.3 单测:嵌套列表缩进层级、引用前缀、hr 行;标题样式。

## 4. 代码块 + syntect 高亮

- [ ] 4.1 `std::sync::LazyLock` 全局 `SyntaxSet::load_defaults_newlines` + `ThemeSet::load_defaults`;`syntect_color_to_ratatui(syntect::highlighting::Color{r,g,b,a}) -> ratatui Color`(取 rgb);按 `theme.is_dark` 选 syntect 主题(暗 `base16-ocean.dark` / 亮 `InspiredGitHub`,均在默认集,缺则回退)。
- [ ] 4.2 围栏码块:info string 首词→语言(未知退 plain);`HighlightLines::highlight_line` 逐行→span(**返回 `Result`,`unwrap_or` 退 plain 不 panic**,保半截 markdown 安全);块加 `bg_sunken` 底 + 语言标签(text_muted);行超宽**按 `char_width` 显示宽截断 + `…`**(预留 `…` 宽、不半切宽字符;v1)。
- [ ] 4.3 单测/快照:` ```rust\nfn main(){}\n``` ` 渲染含高亮 span(关键字/标识符 fg 不同)+ 底色 + 语言标签;未知语言退化为 plain 底色。

## 5. 简易 GFM 表格

- [ ] 5.1 表格事件累积成 `rows: Vec<Vec<Vec<Span>>>`;算各列最大**显示宽**(`display_width`,封顶总宽内);渲表头(text_title+BOLD)+ 分隔行(border_subtle `─`)+ 数据行,按列宽左对齐填充;不做单元格内换行(超列宽**按 `display_width` 截断 + `…`**,预留宽、不半切;**填充与截断同用 `display_width` 保对齐**)。
- [ ] 5.2 单测/快照:2×2 表(含 CJK 单元格)列对齐正确;**另测含需截断的 CJK 单元格(如列宽 6、单元格 5 个 CJK=10 列)→ 渲染后该单元格 `display_width==6`、以 `…` 结尾、无半宽字符、同列各行列位对齐**。

## 6. 接线 transcript + 高度核算

- [ ] 6.1 `src/tui/render.rs` `transcript_lines` 的 `Assistant` 臂改调 `render_markdown`(总宽 − `◆ ` marker 宽;首行 marker、续行缩进包在 Assistant 臂);`transcript_line_count`/滚动/高度随新行数自洽(复用同一 `render_markdown`,避免行数与渲染不一致)。
- [ ] 6.2 回归:纯文本 assistant(无 md 语法)渲染尽量贴近现状;含 md 的既有快照 review diff 后重接受(确认非渲染回归)。

## 7. 校验

- [ ] 7.1 `cargo test --lib` 全绿 + `cargo clippy --all-targets -- -D warnings` 零警告 + `openspec validate add-markdown-render --strict` 通过;新增 markdown insta 快照(暗+亮双主题、含标题/加粗/行内码/围栏码高亮/列表/引用/表格/链接的富消息);既有非-assistant 快照零 churn,assistant 快照 churn 已 review。
- [ ] 7.2 **真机复核**:agent 回一段含代码块的 markdown → 标题/加粗/列表/代码高亮/表格显示正确;暗亮主题各看一眼;长代码块流式不明显卡顿(卡则记 D6+ 缓存);选区复制取渲染文本符合预期。
