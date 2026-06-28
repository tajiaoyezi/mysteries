## Context

- `9495ccd`(feat: 工具卡折叠视觉增强)已 commit:`render.rs` 折叠分组(`transcript_lines` 按相邻 Tool 判 `is_group_first` / 组内不插空行)+ `tool_card_head` 折叠态去 `┌─` + `collapsed_tool_summary` 组首追加 ` · ctrl+o 展开`;配单元测试 `collapsed_group_first_tool_shows_ctrl_o_expand_hint_on_summary_only` + 快照 `tui_tool_group_ctrl_o_hints`(及 8 张迁移)。
- 当时为裸 feat、未走 OpenSpec → **code ⊃ spec**。本 change 据实补 spec,**不改代码**。

权威次序:code / 测试为准,以 `9495ccd` 实际行为书写 spec。

## Goals / Non-Goals

**Goals:** `tui-shell` 折叠 requirement 与 `9495ccd` 实际渲染一致。

**Non-Goals:** 不改代码、不回退、不新增功能;不动折叠 toggle / `ctrl+o` 键位逻辑(`improve-tui-interaction` 已定)。

## Decisions

- **MODIFY 既有折叠 requirement(非新增)**:去边框 / 分组 / 组首 hint 都是对「折叠渲染」同一 requirement 的细化,归并一处,避免拆散折叠语义。
- **port/adapt 归类(供评审)**:
  - 组首 `ctrl+o` 展开提示 = **adapt**:补 `improve-tui-interaction` defer 的 C1 展开可发现性,沿用 `text.muted`、§9 非致命语义。
  - 折叠去 `┌─` 边框 + 连续卡分组紧凑 = **adapt**:`设计规范/03` C5 折叠态的终端版式精简(单行更紧凑,边框留给展开态的体/脚结构)。

## Risks / Trade-offs

- 追认窗口:`9495ccd` 先于本 spec 落地,code/spec 短暂不同步;本 change 关闭该窗口。**教训**:后续同类裸 feat 应在实现同批补 spec,不再事后追认。
