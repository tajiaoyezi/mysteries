# Tasks — add-diff-highlight

## 1. 渲染实现(TUI 外壳,事后测试)

- [ ] 1.1 `card_diff_lines(card, theme, width) -> Vec<Line>`:`compute_diff` 驱动;`│ ` 前缀 + `+ `/`− `/`  `(Ctx 防御)标记;fg = success/error/text_body、bg = bg_base;超宽 `wrap_text` 折行(续行两空格占位、同色);超 `DIFF_MAX_ROWS = 24`(具名常量)止步 + 尾行 `⋯ 其余 N 行`(muted);空 diff 返回空 Vec
- [ ] 1.2 `tool_card_lines` 接线:展开态 diff 体插头行后、output 前;edit/write 展开头行 args 改用 `tool_args_preview`
- [ ] 1.3 `collapsed_tool_summary`:`Done` 且 diff 非空 → ` · +A −D ⌄`(+A success、−D error、零侧省略),置于 exit 分支后、行数分支前;`Running` / `Error` 不变

## 2. 测试

- [ ] 2.1 单测:cap 边界(24 / 25)、空 diff 零侵入、折叠计数仅 Done、零侧省略
- [ ] 2.2 新快照(midnight):edit_file 展开 diff 体、write_file 展开全 Add、折叠 `+A −D`、超限截断
- [ ] 2.3 churn 核对:除 `tui_tool_card_expanded_done`(有意,头行 preview 化)外既有快照零 churn(`git status --short` 证据)

## 3. 门禁

- [ ] 3.1 `cargo test --lib` 全绿;`cargo clippy --all-targets -- -D warnings` 零警告
- [ ] 3.2 `openspec validate add-diff-highlight --strict` 通过
- [ ] 3.3 真机:edit/write 卡折叠见 `+A −D`、ctrl+o 展开见着色 diff、长 content 截断尾标(主 agent / 用户执行)
