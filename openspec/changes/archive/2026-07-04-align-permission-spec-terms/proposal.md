# align-permission-spec-terms

## Why

5 个 active spec 的 requirement 仍残留二元权限时代的旧概念名 `RequiresConfirmation`,而 code 早已实现三态 `PermissionLevel::{ReadOnly, Edit, Execute}`(`src/tool/mod.rs`、`src/permission/mod.rs`,均已 archived 并有测试锁定)。按「code / 编译器 / 测试 > spec」权威次序,spec 术语应追平已实现现实。本 change 做 1.1 工程门面对齐:spec 权限术语按工具真实 level 归位 + `Cargo.toml` 版本号 `0.1.0` → `1.1.0`(反映 1.0 feature-complete 后已积累大量 1.1.x 增量的里程碑现状)。

## What Changes

1. **builtin-tools**:3 个变更类工具的权限级别注解按 code 真实 level 对齐——`write_file` / `edit_file` → `Edit`、`run_shell` → `Execute`(Requirement 标题 + 正文 + Purpose 概述)。
2. **agent-loop / tui-shell / cli-runtime / permission-gate**:泛指「需确认工具」的叙述性用词 `RequiresConfirmation` → 「非 `ReadOnly`(`Edit` / `Execute`)」(过时概念名对齐,非行为变更)。
3. **Cargo.toml**:`version` `0.1.0` → `1.1.0`。

**机制**:纯文档对齐,零 code 行为变更。openspec 的 MODIFIED 按 Requirement 标题精确匹配、改不了标题;RENAMED / REMOVED+ADDED 能改标题但会把该 requirement 移到 spec 文件末尾(打乱顺序),且 delta 无法覆盖 Purpose(overview)。故采**手改 specs/ 保序对齐**(标题 + 正文 + scenario + Purpose 一次到位、diff 最小)+ `archive --skip-specs`(官方 doc-only 通道)。change 内 delta 聚焦 builtin-tools 3 工具的实质权限级别变更(完整 MODIFIED,新标题在 skip-specs 下经 validate 格式校验、不做标题匹配);4 处叙述性措辞对齐以手改 git diff 为完整事实源(避免为改单词复制整段长 requirement)。

## Impact

- Affected specs:builtin-tools(权限级别注解,实质)、agent-loop / tui-shell / cli-runtime / permission-gate(叙述性概念名对齐);**均无行为变更**,现有测试零影响。
- Affected code:无(仅 `Cargo.toml` 版本号元数据)。
- 归档以 `--skip-specs`(doc-only);spec 结构经 `openspec validate --specs` 保绿,change 经 `openspec validate --strict` 保绿。
