# 2026-07-05 · 51 · archive-add-command-allowlist

## 决策
- §13 路线 **1.3 权限工效** v1 = **allowlist + always-allow** | 弃:风险分级(拆后续 change)、危险命令硬拒(留后续) | 主导:用户拍板 | 依据:spec
- PolicyEngine 接 `ChannelDecider::decide` **最前**(allowlist → auto_allows → 询问) | 选:放具体 decider、`gate()`/agent-loop/`PermissionDecision`/`auto_allows` **零改** | 弃:扩 `gate()` 签名注入 policy(改面更大 + 污染 headless 核) | 依据:code
- permission key = **精确规范化命令串、仅 `Execute` 级 + `command`** | 弃:prefix/glob(引匹配语法+安全边界,留后续)、仅凭 `command` 字段无 level gate(自审 F1:防带 command 的非执行工具误 allowlist) | 依据:code/tests
- 弹窗回复 **`PermissionReply{AllowOnce,AllowAlways,Deny}`**;`PermissionDecision{Allow,Deny}` 不变(decide 内部映射 AllowAlways→落盘→Allow) | 弃:给 `PermissionDecision` 加变体(波及 gate/agent-loop 全 match、语义污染) | 主导:审查收敛
- always-allow **写 user config `allowed_commands`**;两层 merge = **并集 dedup**(信任叠加、**显式例外**于字段级 override);持久化失败 → `Notice`、内存仍 remember、仍 Allow | 依据:code/tests
- **附带修既有 render bug**:C6 权限框 diff 无 cap → 长 diff 裁掉 `[y·允许][n·拒绝]` 动作行;按 `area.height-7` 截断「⋯ 其余 N 行」保动作行可见 | 选:**并入本 change**(树共用 `render_permission`、C6 同域、无交互式 add 拆不开 commit;且新增 `[a]` 本就该配不裁的框) | 主导:用户拍板(先修再推进)
- fmt 第 4 次重演 + **flip-flop**(本次 fmt 撤上次 af1e8bc 已接受的 import 排序) | 选:**接受本次 contained fmt + 紧接立 canonical fmt 基线(独立 chore)根治** | 弃:手工剥离(重建 executor 正确改动有风险、且下次重演) | 主导:用户拍板 | 本次仅还原 8 个零内容 CRLF 噪声文件

## 变更
- code:`src/permission/mod.rs`(`PolicyEngine`/`normalize`/`PermissionReply`;`gate`/`PermissionDecision`/`auto_allows` 未动);`src/config/mod.rs`(`allowed_commands` 字段/resolve/并集 merge/`append_allowed_command`);`src/tui/channel.rs`(`ChannelDecider` 三段 decide + policy/user_config_path、`PermissionRequest.allow_always_key` + responder 换 `PermissionReply`);`src/tui/app.rs`(`a` 键 → `AllowAlways`、gated on key);`src/tui/render.rs`(权限框 `[a·总是允许]` + diff 按框高截断);`src/tui/mod.rs`(从 `config.allowed_commands` 建 PolicyEngine 注入)
- spec:permission-gate(ADD:命令 allowlist 自动放行 + always-allow 记忆持久化);config-layering(ADD:`allowed_commands` 并集 merge);tui-shell(MODIFY:ChannelDecider oneshot 往返 → PermissionReply;ADD:命令类权限框 always-allow 选项 + 权限框 diff 按框高截断)
- 快照:新增 `tui_permission_state_allow_always`、`tui_permission_state_long_diff_truncated`

## 待决 / 已知限制
- **fmt canonical 基线 chore**(下一步:全仓 `cargo fmt` + `rustfmt.toml`,终结 flip-flop)
- `config` 依赖 `permission::normalize`(自审 N2,接受;`normalize` 可后挪 util)
- `append_allowed_command` 经 `toml::to_string_pretty` 整体重写 config、抹手写注释(F4,同既有 `write_config`)
- 精确匹配偏窄(`git status` ≠ `git status -s`);prefix/glob 与 per-path 编辑白名单留后续
- 权限框框极矮时 diff 全截为「⋯」(优先保动作行,graceful degrade)

## 引用
- OpenSpec change:add-command-allowlist(permission-gate / config-layering / tui-shell deltas)
- 跨越 session log:本 session(3.1 停点审查 + 3.2/4.x 收尾两轮亲验 + fmt flip-flop 决策 + box-fix 折叠);复用 permission-gate 既有 `gate`/`auto_allows`、config `write_config` 范式
