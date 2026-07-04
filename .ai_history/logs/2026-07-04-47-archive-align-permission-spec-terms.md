# 2026-07-04 · 47 · archive align-permission-spec-terms

## 决策
- spec 权限术语对齐 code 三态 + 版本号 | 选:手改 specs/ 保序 + delta 聚焦 builtin-tools + `archive --skip-specs` | 弃:纯 delta apply(openspec RENAMED / REMOVED+ADDED 改标题会把 requirement 移到文件末尾致乱序;MODIFIED 按标题精确匹配、改标题即 not found;delta 盖不到 Purpose overview)| 主导:主 agent openspec 机制实证 | 依据:openspec 1.4.1 探针实测(probe-terms 证 MODIFIED 改标题 not found、probe-rename 证 RENAMED 移位)
- 版本号 `0.1.0` → `1.1.0` | 选:1.1.0(反映 1.0 feature-complete 后大量 1.1.x 增量的里程碑现状)| 弃:1.0.0(仅补正既成事实)、本轮不动 | 主导:用户拍板 | 依据:里程碑史([[2026-06-27-14-archive-finish-1-0]])
- 权限级别按工具真实 level 归位:`write_file` / `edit_file` → `Edit`、`run_shell` → `Execute`;4 处泛指叙述(agent-loop / tui-shell / cli-runtime / permission-gate)→ 非 `ReadOnly`(`Edit` / `Execute`)| 依据:code(`PermissionLevel` 三态,`src/tool/mod.rs`、`src/permission/mod.rs`,权威次序 code > spec)

## 变更
- 手改 5 specs/ 13 处旧概念名 `RequiresConfirmation` 对齐;`Cargo.toml` `0.1.0` → `1.1.0`
- change delta:MODIFIED builtin-tools 3 工具(完整新内容,skip-specs 下经 validate 格式校验、不做标题匹配);4 处叙述性措辞对齐以手改 git diff 为完整事实源
- 验证:术语清零(grep 无匹配)、`validate --specs` 15/0、change `--strict` valid、`cargo check`(`mysteries v1.1.0`)、`test --lib` 585 零回归、git diff 人工逐处核对

## 待决
- `cargo build` 链接期 `mysteries.exe`(用户 TUI 进程)占用致 os error 5;编译正确性已由 `cargo check` + `test --lib` 旁证,exe 打包待进程释放,非代码问题

## 引用
- OpenSpec change:align-permission-spec-terms
- 前置:[[2026-07-04-46-archive-polish-paste-fold]](记「1.1 工程对齐为后续件」)
- 同批随后归档:[[2026-07-04-48-archive-fix-paste-latency]](粘贴提速,先完成后归档;因 tui-shell delta 与本 change 手改同文件,本 change 先落地避免 commit 交织)
