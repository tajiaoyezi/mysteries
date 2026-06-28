## Why

`9495ccd`(feat: 工具卡折叠视觉增强)已落地三项折叠态渲染增强——折叠去 `┌─` 边框、连续工具卡分组不插空行、组首追加 ` · ctrl+o 展开` 提示(补 `improve-tui-interaction` 中 defer 的展开可发现性)。但当时为**裸 feat、未走 OpenSpec**,其行为已**超出** `improve-tui-interaction` 折叠 requirement 的描述,造成 **code ⊃ spec**。

本 change 据实**补 spec**(权威次序 code > spec):不改代码、不回退,只让 `tui-shell` 折叠 requirement 与 `9495ccd` 的实际渲染一致,关闭 code/spec 不同步窗口。

## What Changes

- `tui-shell`: **MODIFY**「工具输出折叠与全局展开(ctrl+o)」,补三条已实现行为:
  - ① 折叠态单行头**不含** `┌─` 边框前缀(`┌─` / `└─` 边框仅展开态用);
  - ② 连续 `Tool` 块视为一组,**组内相邻块之间不插空行**(紧凑),组边界仍插空行;
  - ③ 折叠态每个连续 `Tool` 组**仅组首**在结果摘要后追加 ` · ctrl+o 展开`(`text.muted`),组内非首与展开态不追加。

## Capabilities

### Modified Capabilities
- `tui-shell`: **MODIFY**「工具输出折叠与全局展开(ctrl+o)」—— 折叠去边框 + 连续卡分组 + 组首 `ctrl+o` 展开提示(据 `9495ccd` 补 spec)。

## Impact

- **code**:无(`9495ccd` 已实现;本 change 仅补 spec,不改一行代码)。
- **spec**:`openspec/specs/tui-shell/spec.md` 折叠 requirement。
- **验证**:`9495ccd` 已带单元测试(`collapsed_group_first_tool_shows_ctrl_o_expand_hint_on_summary_only`)+ 快照(新 `tui_tool_group_ctrl_o_hints` + 8 张迁移),本 change 确认 spec ↔ test 对齐。
- **deps**:无。
