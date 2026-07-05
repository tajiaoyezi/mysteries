# add-command-allowlist

## Why

§13 路线 **1.3 权限工效**:当前权限门只有 `PermissionMode × PermissionLevel` 粗粒度矩阵——`Normal` 下每条 `Edit`/`Execute` 都逐条弹窗,唯一省事办法是切 `AcceptEdits`/`Yolo` 一刀切全放,**没有中间档**。用户无法表达「`git status`/`cargo build` 这类总是放行、但 `rm` 仍问我」。对标 Claude Code 的 allowed-tools:在「问」之前插一层 `PolicyEngine`,按命令白名单自动放行,并支持弹窗时「总是允许」持久记忆。

v1 **只做 allowlist + always-allow**(用户已拍板),不做风险分级(留后续 change)。

## What Changes

1. **PolicyEngine 命令白名单**:`ChannelDecider::decide` 在查 `auto_allows(mode)` 与发起 UI 询问**之前**,先查 `PolicyEngine`——工具调用的 **permission key**(v1:shell/`Execute` 工具 `args["command"]` 规范化串,trim + 压内部空白)命中白名单 → 直接 `Allow`、不弹窗。非命令类工具(如 `Edit` 的 `path`)无 key、永不命中(仍走既有 mode/询问)。`gate()` 与 agent-loop **零改动**(经既有 `PermissionDecider` seam 接入)。
2. **always-allow 弹窗选项 + 持久化**:存在 permission key 时,C6 权限框加 `[a·总是允许]`;选中 → 允许本次 + 把 key 记入内存 PolicyEngine + **写回 user config 的 `allowed_commands`**;下次同命令自动放行。弹窗回复升 `PermissionReply { AllowOnce, AllowAlways, Deny }`;`gate()` 返回仍是 `PermissionDecision { Allow, Deny }` 不变(decide 内部把 `AllowAlways` 落盘后返回 `Allow`)。responder 断开仍 `Deny`(fail-safe 不变)。持久化失败 → 非致命 `Notice`、内存记忆仍生效。
3. **config `allowed_commands` 字段**:`RawConfig` 加 `allowed_commands: Option<Vec<String>>`(`#[serde(default)]`),resolve 为 `Config.allowed_commands: Vec<String>`(默认空);user/project 两层 **并集 merge**(信任叠加,区别于字段级 override);always-allow 只写 user 层、dedup。

## Impact

- 修改 capability:`permission-gate`(ADD:命令 allowlist 自动放行 + always-allow 记忆持久化);`config-layering`(ADD:`allowed_commands` 并集 merge + append 写回);`tui-shell`(MODIFY:`ChannelDecider 权限 oneshot 往返` 的 responder 升 `PermissionReply` + 请求带 key;ADD:命令类权限框 `[a·总是允许]` 选项)
- Affected code:`src/permission/`(新 `PolicyEngine` + `PermissionReply`;`gate()`/`PermissionDecision` 不变);`src/tui/channel.rs`(`ChannelDecider` 持 policy + user_config_path、decide 三段判定、`PermissionRequest` 带 `allow_always_key` + responder 换 `PermissionReply`);`src/config/mod.rs`(`allowed_commands` 字段/resolve/并集 merge/`append_allowed_command` 写回);`src/tui/app.rs` + `src/tui/render.rs`(权限框 `[a]` 选项 + `a` 键 → `AllowAlways`);装配点(从 `config.allowed_commands` 建 PolicyEngine 注入 `ChannelDecider`)
- 附带修复(既有 render bug,与 always-allow 同触 `render_permission`):C6 权限框 diff body 无 cap,长 diff(大文件 write）把 `[y·允许][n·拒绝]` 动作行挤出框外裁掉 → 按 `area.height` 预留动作行、超出截断「⋯ 其余 N 行」(见 tui-shell delta「权限框 diff 按框高截断」)
- **无新依赖**(命令规范化用 std;写回复用既有 `toml`)
- 回退:allowlist 空(默认)时行为与现状**完全一致**(直落 `auto_allows`/询问);always-allow 是纯增选项;`allowed_commands` 缺省即空
