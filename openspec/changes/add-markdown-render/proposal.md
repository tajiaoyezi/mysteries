## Why

agent 输出本是 markdown,但 TUI 现在把 `Assistant` 块当**纯文本**渲染:`transcript_lines` 的 `Assistant` 臂走 `message_lines("◆ ", text, 单一 text_body 样式)`(render.rs:352),`wrap_text` 按字符宽换行,无任何 markdown 解析——标题、加粗、行内/围栏代码、列表、表格全是灰扑扑一片,可读性差。渲染加法线程的第二件(继粘贴折叠之后):让 assistant 输出按 markdown 上色,代码块带语法高亮。

## What Changes

- **新模块 `src/tui/markdown.rs`**:`render_markdown(text: &str, theme: &Theme, width: usize) -> Vec<Line<'static>>`,`transcript_lines` 的 `Assistant` 臂改调它(保留 `◆ ` marker + 续行缩进);其余块不动。
- **解析:`pulldown-cmark`**(CommonMark + GFM 表格/删除线)。走事件流,把每个元素映射到 `Theme` 语义色 + `Modifier`(标题 bold、`**bold**`、`*italic*`、`inline code` 区分 bg、列表 marker+缩进、`> 引用`、`---` hr、链接 accent+下划线)。
- **代码块语法高亮:`syntect`**(启 `default-fancy` feature = **纯 Rust regex-fancy 引擎**、不引 C `onig`;`std::sync::LazyLock` 懒加载 `SyntaxSet`/`ThemeSet` 一次,无需第三方)。按语言逐 token 上色,syntect `Color` 转 ratatui `Color`;按 `Theme.is_dark` 选暗/亮 syntect 主题;带语言标签 + 区分底色。
- **`Theme` 加 `is_dark: bool`**(midnight=true / daylight=false),供 syntect 选主题(及个别元素明暗取舍)。
- **span 感知换行**:markdown 行内是带样式的 span 序列,需按 width 换行且保留样式(含 CJK 宽度,复用 `width::char_width`)——比现有纯串 `wrap_text` 复杂,新写一个 span 版换行器,**不碰** `visual_input_layout`。
- **简易 GFM 表格**:按列宽对齐渲染(表头 + 分隔 + 行;不做单元格内换行/列合并)。

## Capabilities

### New Capabilities

- `tui-shell`:
  - **ADDED**:`Assistant 消息 markdown 渲染(CommonMark+GFM,代码块语法高亮)` —— `render_markdown` 解析 + 元素上色 + span 换行 + syntect 代码高亮 + 简易表格;仅作用于 `Assistant` 块;流式每帧重渲。

## Impact

- **依赖(2 个新增,均 TUI 层,核心自实现不碰;新增依赖理由见 design)**:
  - `pulldown-cmark`:CommonMark+GFM 解析(全量手写不现实)。
  - `syntect = { version = "5", default-features = false, features = ["default-fancy"] }`(纯 Rust regex-fancy、无 C onig;含默认 syntax/theme dump):代码块语法高亮(标准方案,agent 代码输出高价值)。懒加载用 `std::sync::LazyLock`(标准库,不新增依赖)。
- **代码**:
  - 新 `src/tui/markdown.rs`(解析→带样式 `Line`、span 换行、syntect 桥接、表格)。
  - `src/tui/render.rs`:`Assistant` 臂改调 `render_markdown`;`transcript_line_count`/高度核算随之走新行数。
  - `src/tui/theme.rs`:`Theme` 加 `is_dark`(midnight/daylight 各设)。
  - `Cargo.toml`:加两依赖。
- **测试**:`render_markdown` 各元素 span/样式单测 + markdown-rich assistant 消息 insta 快照(暗/亮双主题)——TUI 外壳走**事后**快照,无 red-green。
- **风险**:syntect 体积/加载、流式重高亮成本、span 换行宽字符边界、syntect 主题与双调色板观感、assistant 快照 churn。见 design。
- **Non-Goals(v1)**:代码块行号、链接可点击/OSC8、表格单元格内换行与列合并、深层嵌套引用样式、数学公式、运行时 `/theme` 切换。
- **边界**:同属"`tui/` 渲染加法线程";**diff 高亮**是该线程第三件、另开 change。
