# Tasks — align-permission-spec-terms

纯文档对齐,无 code 行为变更、无 TDD。手改 specs/ 保序(openspec 改标题会乱序 + 盖不到 Purpose),archive 用 `--skip-specs`。执行 agent MUST NOT:改动任何 `src/` 逻辑、勾选未完成的门禁项。

## 1. spec 术语对齐(手改 specs/,按 code 真实 level)

- [x] 1.1 `builtin-tools`:`write_file` / `edit_file` 的 Requirement 标题 + 正文「权限级别」→ `Edit`;`run_shell` 标题 + 正文 → `Execute`;Purpose 概述第一段「3 个变更类工具(…,`RequiresConfirmation`)」→ 按真实 level 标注(`Edit` / `Execute`)
- [x] 1.2 `agent-loop`:「结构化观测事件」正文「`RequiresConfirmation` 工具询问前 `WaitingForPermission`」+ scenario「权限拒绝仍上报工具完成」的「某 `RequiresConfirmation` 工具」→ 「非 `ReadOnly`(`Edit` / `Execute`)工具」
- [x] 1.3 `tui-shell`:「agent-task 一轮编排」正文「含 `RequiresConfirmation` 工具的脚本」+ scenario「一个 RequiresConfirmation 工具的 tool_call」→ 「非 `ReadOnly`(`Edit` / `Execute`)工具」
- [x] 1.4 `cli-runtime`:「stdin y/n 权限 decider」正文「对 `RequiresConfirmation` 工具」→ 「对非 `ReadOnly`(`Edit` / `Execute`)工具」
- [x] 1.5 `permission-gate`:「拒绝产出 is_error ToolResult」scenario「Loop 处理一个 `RequiresConfirmation` 的 tool_call」→ 「一个 `Edit` / `Execute`(非 `ReadOnly`)的 tool_call」

## 2. 版本号

- [x] 2.1 `Cargo.toml` `version = "0.1.0"` → `version = "1.1.0"`

## 3. delta 记录(仅 builtin-tools 实质变更)

- [x] 3.1 `specs/builtin-tools/spec.md` delta:MODIFIED `write_file`(Edit)/ `edit_file`(Edit)/ `run_shell`(Execute),完整新内容(标题 + 正文 + scenarios),与手改 specs/ 一致

## 4. 门禁

- [x] 4.1 `openspec validate align-permission-spec-terms --strict` 通过
- [x] 4.2 `openspec validate --specs` 通过(15/0);`RequiresConfirmation` 在 `openspec/specs` 下 5 处目标清零(grep 无匹配)
- [x] 4.3 编译正确性:`cargo check` 通过(`mysteries v1.1.0`)+ `cargo test --lib` 585 零回归;`cargo build` 的 exe 链接受用户 TUI 进程占用(os error 5)阻塞,待进程释放后完成打包(非代码问题,不 kill 用户进程)

## 5. 归档

- [x] 5.1 `openspec archive align-permission-spec-terms --skip-specs -y`;附决策记录 log 47(同一提交)
