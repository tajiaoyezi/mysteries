# Tasks — refine-tool-card-folding

> 追认性质:code 已在 `9495ccd`,本 change **只补 spec + 确认 test↔spec 对齐**,无红绿、不改代码。

## 1. spec 补齐

- [x] 1.1 **MODIFY** `tui-shell`「工具输出折叠与全局展开(ctrl+o)」:补 ① 折叠去 `┌─` 边框 ② 连续 `Tool` 分组不插空行 ③ 组首追加 ` · ctrl+o 展开`(header byte-for-byte 对齐原 spec)。
- [x] 1.2 确认 spec 更新/新增的 scenario ↔ 既有测试对齐:`collapsed_group_first_tool_shows_ctrl_o_expand_hint_on_summary_only`(单元)、`tui_tool_group_ctrl_o_hints`(快照)、scenario「默认折叠为单行」措辞已反映去边框 + 组首 hint。

## 2. 验证

- [x] 2.1 `openspec validate refine-tool-card-folding --strict` 通过。
- [x] 2.2 `cargo test` 折叠相关单元 + 快照保持绿(确认 spec 描述与 `9495ccd` 实际渲染一致)。
