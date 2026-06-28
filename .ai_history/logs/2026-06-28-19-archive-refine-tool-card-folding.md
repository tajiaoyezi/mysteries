# 2026-06-28 · 19 · archive refine-tool-card-folding(据实补 9495ccd 折叠增强 spec)

## 决策
- 9495ccd 折叠视觉增强(去 ┌─ 边框 + 连续卡分组 + 组首 ctrl+o 展开提示)为裸 feat 未走 OpenSpec → code ⊃ spec;本 change 据实补 spec、不改代码 | 选:MODIFY 既有折叠 requirement(归并折叠语义)| 弃:ADD 新 requirement(拆散折叠语义)| 主导:用户(要追认)+ 主 agent | 依据:code(9495ccd)> spec
- port/adapt:组首 ctrl+o 提示 = adapt(补 improve-tui-interaction defer 的 C1 可发现性);折叠去边框 + 分组 = adapt(C5 折叠态版式精简)

## 变更
- spec tui-shell MODIFY「工具输出折叠与全局展开(ctrl+o)」:+去边框 / +连续卡分组不插空行 / +组首 ctrl+o 展开;scenario「默认折叠单行」措辞更新 + 新增「连续工具卡分组紧凑且仅组首带提示」
- code:无(9495ccd 已实现)
- 验证:既有折叠单元测试 + tui_tool_group_ctrl_o_hints 快照绿;validate --strict 过

## 待决
- 教训:裸 feat(9495ccd)事后追认 spec —— 后续同类改动应实现同批补 spec,不再事后补
- 承前未动:1.1 第三步 add-token-compaction;git 身份 wanglei30 临时;leafiellune purge

## 引用
- change:refine-tool-card-folding(archive 2026-06-28-*);据实补 commit 9495ccd
- 关联:improve-tui-interaction(16,折叠 + ctrl+o 本体,C1 defer);本 change 补 C1
- session:本会话——9495ccd 审查 + 单独 commit + 追认 spec
