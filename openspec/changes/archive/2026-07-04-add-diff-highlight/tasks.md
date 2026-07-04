# Tasks — add-diff-highlight

## 1. 渲染实现(TUI 外壳,事后测试)

- [x] 1.1 纯函数 `diff_body_lines(diff: &[DiffLine], theme, width) -> Vec<Line>`:内容宽 = `width.saturating_sub(4).max(1)`(`│ ` 2 列 + 标记 2 列,**勿照抄 output 的 −2**);`+ `/`− ` 标记(`−` 用 U+2212)fg = success/error、bg = bg_base、`│ ` 前缀 fg = border_subtle;超宽用既有 `wrap_text` 折行,续行 `│ ` + 两空格占位、同色、不重复标记;**屏行预算** `DIFF_MAX_ROWS = 24`(按折行后显示行计,允许切在逻辑行中途),超出止步 + 尾行 `⋯ 其余 N 行`(N = 未完整显示的 DiffLine 数,muted,带 `│ ` 前缀);空 diff 返回空 Vec
- [x] 1.2 `tool_card_lines` 接线:展开态以 `compute_diff` 结果调 `diff_body_lines`,插头行后、output 前;edit/write 展开头行 args **无条件**改用 `tool_args_preview`(缺 path 走既有 JSON fallback)
- [x] 1.3 `collapsed_tool_summary`:新分支**显式判 `status == Done`** 且 diff 非空 → ` · +A −D ⌄`(+A success、−D error、` · `/` ⌄` text_secondary、`−` U+2212、零侧双向省略),插于 exit 分支后、行数分支前;`Running` / `Error` 行为不变

## 2. 测试

- [x] 2.1 单测(`diff_body_lines` 直测,无需渲染):屏行 cap 边界(预算恰满无尾行 / 超 1 行 / 单条超长行中途截断 N=1)、续行前缀且不重复标记、各屏行显示宽 ≤ width、空 diff 空 Vec;折叠计数仅 `Done`(`Error` 不显)、零侧省略双向(+only 与 −only)。空 diff 零侵入断言口径:不产 diff 体行 + 折叠摘要不变(**不含展开头行**——其变更由 preview 规则单独锁定)
- [x] 2.2 新快照(midnight,对照 spec delta 各 scenario):edit_file 展开 diff 体、write_file 展开全 Add、折叠 `+A −D`(三卡:+3−2 / +12 / −2)、短行超限截断(30 行→24+尾行,用 `render_to_styled_with_size` 使整卡入帧,如 80×40)、单条超长行屏行截断、窄视口(40 列)CJK 折行、Running/Error 展开不分态。**MUST NOT 修改既有 `tool_card` 助手(render.rs:2086)的签名与默认 args**——新 scenario 内联构造 `ToolCard`(参照 `run_shell_exit_foot_snapshot` 内联写法)或新增独立助手
- [x] 2.3 churn 核对:`git status --short` 预期 = **恰 1 个修改 .snap**(`tui_tool_card_expanded_done`,头行 preview 化)+ 新增 .snap 若干,无其他修改(原文贴报告)

## 3. 门禁

- [x] 3.1 `cargo test --lib` 全绿;`cargo clippy --all-targets -- -D warnings` 零警告
- [x] 3.2 `openspec validate add-diff-highlight --strict` 通过
- [x] 3.3 真机:edit/write 卡折叠见 `+A −D`、ctrl+o 展开见着色 diff(用户截图证实:`+21 −2 ⌄` 计数、展开红绿 diff、头行 preview 化);minified 单行未真机单验,由快照 `tui_tool_card_diff_long_line_truncated` 与同一截断 machinery(真机已验 8+15)覆盖

## 4. 真机反馈修订:diff 恒显、与 tools_expanded 解耦(D6)

- [x] 4.1 实现:`tool_card_lines` 折叠分支在单行头之后追加 diff 体(仅 write/edit 且 diff 非空);屏行预算参数化——折叠 `DIFF_COLLAPSED_MAX_ROWS = 8` / 展开 `DIFF_MAX_ROWS = 24`(均具名常量,`diff_body_lines` 增 max_rows 参数);折叠态仍不渲 output / 脚 / `┌─└─` 边框,diff 行保留 `│ ` 前缀
- [x] 4.2 单测:折叠配额截断(12 行 → 8 + `⋯ 其余 4 行`)、折叠 `Running` / `Error` 亦渲 diff 体、非 write/edit 与空 diff 折叠卡行集零变化(仍单行)
- [x] 4.3 快照:更新 `tui_tool_card_collapsed_diff_counts`(体现折叠 diff 体 + 12 行卡截断到 8);churn 判据更新为「change 前既有 .snap 仅 `tui_tool_card_expanded_done` 修改;本 change 内新增 .snap 可更新」;`git status --short` 原文贴报告
- [x] 4.4 门禁:`cargo test --lib` 全绿、`cargo clippy --all-targets -- -D warnings` 零警告、`openspec validate add-diff-highlight --strict`
- [x] 4.5 真机:默认折叠即见 diff、ctrl+o 展开配额 24、read/run_shell 默认仍单行(用户截图证实:折叠态 diff 体 + `⋯ 其余 15 行`(23 条 = 配额 8 + 其余 15,严丝合缝)、read/glob/grep/run_shell 均单行摘要)
