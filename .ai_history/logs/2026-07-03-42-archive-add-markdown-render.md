# 2026-07-03 · 42 · archive add-markdown-render

## 决策
- **解析用 `pulldown-cmark` 0.13(ENABLE_TABLES|STRIKETHROUGH)、事件流 + inline 样式计数栈** | 弃:手写 parser(CommonMark+GFM 边界巨多) | 依据:design 已论证,实现按 0.13.4 实际事件流补两个兼容点(tight list 文本直接在 `Item` 内→push_text 自动开段落;`TableHead` 无 `TableRow` 包裹→`End(TableHead)` 时 flush header row) | 主导:讨论收敛
- **`SoftBreak`/`HardBreak` 均映射硬换行、不折空格** | 保纯文本 assistant 行结构与旧 `message_lines` 的 `split('\n')` 一致——落地效果:**既有 assistant 快照零 churn**(设计预期"大概率 churn"实际一张没变,兼容做满) | 依据:tests + 快照
- **代码高亮 `syntect 5.3`(`default-features=false, features=["default-fancy"]`,纯 Rust regex-fancy)+ `std::sync::LazyLock`** | 弃:默认 onig C 引擎(跨平台构建险,`cargo tree -i onig` 验证无)、once_cell(std 已稳) | 三层安全回退:未知语言→plain、主题缺失→任意可用主题→plain、`highlight_line` Err→plain,全路径无 panic | 依据:code/tests
- **span 感知换行 `wrap_spans` 新写,不碰 `visual_input_layout`**;截断统一「预留 `…` 显示宽、绝不半切宽字符、填充与截断同用 `display_width`」(代码行/表格单元格同口径) | 依据:tests(`ab你|好` 断行、`一二 …` 精确截断、列宽 6 CJK case)
- **review finding(主 agent 复核,已修)**:执行稿代码行**先截断再喂 `highlight_line`**——`HighlightLines` 跨行状态机被截断源(含追加的 `…`)污染,截在字符串中间会把后续行整块当字符串上色,且 newlines 语法集期望行含 `\n`。修:**先高亮完整行(带 `\n`)再对 span 截断**(复用表格的 `fit_spans_to_width`),新增回归测试锁定 | 严重度 minor(仅超宽行触发,cosmetic)
- **接线**:仅 `Assistant` 臂改调 `render_markdown`(marker `◆ `/续行缩进由臂包,正文 width=总宽−marker 宽);`transcript_line_count` 经 `transcript_content_lines` 复用同一渲染路径,行数与渲染无两套算法 | 依据:code

## 变更
- 新 `src/tui/markdown.rs`(~1040 行含 10 测):事件流状态机、块/inline 样式映射(D2)、`wrap_spans`、syntect 桥接、简易 GFM 表格(列宽收缩 + `│`/`─┼─`)。
- `render.rs`:`assistant_message_lines` + 2 张带样式富消息快照(暗/亮);`theme.rs`:`is_dark`;`Cargo.toml`:+pulldown-cmark、+syntect;`mod.rs`:+1 行声明。
- 执行 agent 曾用 cargo fmt 扫全库产生 11 个纯行尾噪声文件(内容零 diff,主 agent 逐个 `git diff --quiet` 验证后 restore,未入提交)。
- `cargo test --lib` **487 passed** / 0 failed / 2 ignored;clippy 零警告;既有快照零 churn;真机(暗主题)验:标题/加粗/表格 CJK 对齐/行内码/多色代码高亮/流式 ~27 t/s 无破碎。

## 待决
- daylight 主题真机未截图(有带样式快照锁定,低风险);选区复制含 markdown 渲染文本未真机验。
- 流式长代码块若真机卡顿 → D6+ 按 `(text,width,is_dark)` 缓存已完成块(v1 未做,未见卡顿)。
- v1 Non-Goals:代码块行号、链接 OSC8、表格单元格内换行/列合并、深层嵌套引用、数学公式、运行时 `/theme` 切换、syntect scope→调色板精细映射。
- 表格超总宽策略 v1 为「收缩最宽列到 1」,极多列仍可能溢出(ratatui 裁),真机看。

## 引用
- OpenSpec change:`add-markdown-render`(propose ad84fb5)→ archive/2026-07-03-add-markdown-render
- 相关 log:[[2026-07-03-40-archive-add-paste-fold]](同属 tui/ 渲染加法线程;第三件 diff 高亮另开 change)
- 跨越 session:本会话(主 agent review + fix;执行 agent 实现 1.1~7.1)
