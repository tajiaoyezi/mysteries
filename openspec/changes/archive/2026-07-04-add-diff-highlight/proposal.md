# add-diff-highlight

## Why

工具卡对 write_file / edit_file 的呈现仍是原始 args:折叠态只有 path 与输出行数,展开态是整段转义 JSON(`old_string` / `new_string` 挤在一行),变更内容不可读。权限确认框早已有 +/− 着色 diff(`compute_diff`),但真正留在 transcript 里的卡片本体没有。本 change 是 tui/ 渲染线程(markdown → 粘贴折叠 → diff)的收官件,log 42 已排定。

## What Changes

1. **diff 体(恒显,与 tools_expanded 解耦)**:write_file / edit_file 卡片渲染着色 diff(复用 `compute_diff`:`Del` 红 `− ` / `Add` 绿 `+ `,`│ ` 边框前缀、`bg.base` 底、超宽折行,内容宽 = 整行宽 − 4)——**折叠态(默认)也显示**,位于单行头之后,配额 `DIFF_COLLAPSED_MAX_ROWS = 8` 屏行;展开态位于头行与 output 之间,配额 `DIFF_MAX_ROWS = 24`(均按折行后显示行计,minified 单行大文件同样被截住),超配额渲尾行 `⋯ 其余 N 行`。这两个工具展开态头行 args 无条件改用既有 preview(`path=...`),不再整段 JSON(内容由 diff 体承载)。其他工具(read / run_shell 等)折叠行为零变化——对齐 Claude Code 按工具类型分型:diff 默认露出、输出噪音默认收起。
2. **折叠态计数**:`Done` 且 diff 非空的 edit/write 摘要显示 ` · +A −D ⌄`(各自着色,为 0 的一侧省略),替代 ` · N 行 ⌄`;`Running` / `Error` 折叠摘要维持既有(不显计数,防误读为已应用)。
3. **零外溢**:权限框 diff 渲染、`compute_diff` 语义、其他工具卡行为全部不变。

## Impact

- Affected specs: `tui-shell`(MODIFIED「工具卡 C5 渲染」)
- Affected code: `src/tui/render.rs` 为主(`tool_card_lines` / `collapsed_tool_summary` + 新渲染函数);`src/tui/app.rs` 原则上不动(`compute_diff` / `DiffLine` 复用)
- **有意快照 churn(仅此一处,点名)**:`tui_tool_card_expanded_done`——write_file 展开头行 args 由 `{"path":"note.txt"}` 变为 `path=note.txt`;**其余既有快照必须零 churn**
- TUI 外壳改动:事后 TestBackend + insta 快照,无红绿停点
