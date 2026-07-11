# Tasks — add-network-permission-level

执行边界：本 change 不新增 crate / config 字段 / session wire，不做 per-origin allowlist、DLP、并行工具或 MCP。headless 权限内核、tool-owned preview、canonical request builder、redirect budget、共享 layout / input barrier 与 CLI I/O 严格 RED→GREEN；新权限路径首次红灯后停点确认。TUI 外壳走真实事件回归 + 事后 `TestBackend + insta`。所有 Cargo 编译 / 测试 / Clippy / Build 使用 `CARGO_TARGET_DIR=target/codex-network-permission`，不得为解除锁而 kill 用户进程。不得用真实网络驱动自动化测试，不得为过测改写既有快照语义。

## 1. 编译骨架（不实现目标行为）

- [x] 1.1 在 `src/tool/mod.rs` 增 `PermissionLevel::Network`、`NetworkPermissionPreview` / scope 类型和 `Tool::network_permission_preview` 方法；compile stub 暂返回 `authorizable=true` generic preview。给 `WebFetcher` 机械增加带错误占位值的临时 default `permission_scope()`，使所有现有 impl 先编译、后续 conformance 产生运行期 RED；GREEN 时删除该 default并要求 impl 显式声明。在 `src/permission/mod.rs` 增 `PermissionCheck{call, tool, network_preview: Option<&NetworkPermissionPreview>}`、`PermissionDenial::{UserDenied, NetworkUnauthorizable(String)}` 与 `PermissionGateOutcome::{Allow, Deny(PermissionDenial)}`，把 `PermissionDecider` 机械扩为接收 `PermissionCheck`、`gate(Network)` 暂直接返回 `Deny(NetworkUnauthorizable(...))`、`auto_allows(_, Network)=false`、Plan 暂不下发 Network，web 工具仍 ReadOnly；agent-loop 暂把两种 Deny 都映射为既有 user-denied ToolOutcome，留给 §3.5 运行期 RED 推翻。运行 `$env:CARGO_TARGET_DIR='target/codex-network-permission'; cargo test --lib --no-run`，只建立可编译且必被后续 RED 推翻的骨架。

## 2. Preview fail-closed 权限内核（强制 TDD；首次路径停点）

- [x] 2.1 **RED**：在 `src/permission/mod.rs` / `src/tool/mod.rs` 先写运行期失败测试：未 override Network Tool 的 default preview 必须 `authorizable=false` 且 reason 非空；`authorizable=true` 必须有 canonical target / scope 且 reason 为空；Network preview 只计算一次并以 `PermissionCheck.network_preview=Some` 原值传给 decider，Edit / Execute 恒为 None；可授权 Network / Edit / Execute 的用户 Deny 得 `Deny(UserDenied)`，不可授权 preview 即使 decider Deny / Allow / 异常 AllowAlways / Yolo 也得 `Deny(NetworkUnauthorizable(原 reason))` 且 decider可见 reject-only preview；四 mode 中仅 authorizable Network 的 Yolo auto Allow；Network `permission_key=None`。运行 targeted tests，确认至少 default / check-shape / outcome / Yolo 断言红而非编译错。
- [x] 2.2 **停点**：贴出 §2.1 测试代码与原始 RED 输出，等用户确认后再进入 GREEN（新权限路径首次成型，遵 AGENTS.md 折中档）。
- [x] 2.3 **GREEN**：最小实现 Tool default 不可授权、preview invariant、`PermissionCheck` 的 Network Some / 非 Network None、gate 单次 preview 计算 / 传递 / `PermissionGateOutcome` 与返回点 Deny clamp、`auto_allows` Network 矩阵及 command-only PolicyEngine，使 §2.1 全绿；不可授权仍可进入 decider 供 TUI reject-only 呈现，但任何 Allow 都被 gate clamp，且系统不可授权不得伪装成 user denied。
- [x] 2.4 **regression**：重跑 ReadOnly 直放、Edit / Execute decider、command allowlist / AllowAlways 与四 mode 轮转测试，证明新增 preview 参数不改变旧三类行为。

## 3. Web canonical preview、Plan schema 与 agent-loop（强制 TDD）

- [x] 3.1 **RED**：补 `ToolRegistry::schemas_for` 与 builtin level 测试，锁 Plan=`ReadOnly + Network + plan_only`、非 Plan=`ReadOnly + Network + Edit + Execute`、顺序不变，以及 web_fetch / web_search level=Network；运行 targeted tests 得断言红。
- [x] 3.2 **GREEN**：最小修改 Plan schema filter 与两个 web tool level，使 §3.1 转绿；`schemas()`、plan_only 默认及所有非 web builtin level 零回归。
- [x] 3.3 **RED**：为 web_fetch / web_search 的 tool-owned preview 写失败测试：preview canonical target 必须与 capture WebFetcher 实收 URL 相等；web_fetch 覆盖 userinfo、IDN/punycode、数字 IP、IPv6、默认端口；web_search 覆盖空格、中文与 `&`；preview scope 必须逐字段等于当前 tool 所持 `WebFetcher::permission_scope()`。另锁默认生产 registry 只装配 `ReqwestFetcher`，其声明的 `MAX_REDIRECTS` / cross-origin / 逐跳 SSRF scope 与 redirect / SSRF 行为通过 conformance tests 一致；`MockFetcher` 仅作零网络 test double。缺失 / 非 string / 不可解析参数必须不可授权且零 fetch。§1.1 的 tool default preview与错误占位 fetcher scope 应令断言在运行期红，不能是编译错。
- [x] 3.4 **GREEN**：在 `src/tool/web.rs` 抽取 web_fetch 纯 request builder并复用 `ddg_search_url`；移除 `WebFetcher::permission_scope()` 的临时 default，要求 `ReqwestFetcher` / Mock / capture impl 显式实现，让 preview 与 execute 使用同一 canonical URL / DDG target并读取当前 fetcher scope。`ReqwestFetcher` 的 scope 由同一 `MAX_REDIRECTS` 与 cross-origin / 逐跳 SSRF policy 生成，默认生产 registry 只装配该实现，新增生产 fetcher 必须经过 conformance tests。override 两个专用 preview，使 §3.3 全绿；permission / TUI / CLI 层不得按工具名或常量重建 target / scope。
- [x] 3.5 **RED**：在 `src/agent/mod.rs` 用 Mock provider / authorizable Network Tool 锁 Plan Network Allow 可执行、Deny 不 execute / is_error 入 history，Edit / Execute 仍纵深拒；再分别断言 `Deny(UserDenied)` 保持既有 user-denied content，而 `Deny(NetworkUnauthorizable(reason))` 将原 reason 写入 is_error history且 content 不得归因为 user denied。另从 Mock provider 实收首条 System message 直接断言：web research 每次需 Network 授权、不得 Edit / Execute、`submit_plan` 每步带可验收 `validation`、不再把 `web_*` 描述为只读；expected 不得引用 `PLAN_MODE_INSTRUCTION` 常量。运行 targeted tests，确认 Plan 纵深拒 / outcome-to-history / 文案断言均为运行期 RED。
- [x] 3.6 **GREEN**：将 Plan 纵深拒最小收窄为 Edit / Execute，实现 agent-loop 对 `PermissionGateOutcome` 的两类 Deny→ToolOutcome / history 映射，并更新完整 `PLAN_MODE_INSTRUCTION` 三分支 + validation + Network 语义，使 §3.5 全绿；保持 transient-only、Normal 不注入与轮顶 mode snapshot。
- [x] 3.7 **characterization / regression**：锁 observer 的 `readonly == (level == ReadOnly)`；authorizable / 不可授权 Network 均为 false，需要 UI 时出现 WaitingForPermission，最终 Deny 仍 ToolCallFinished(is_error)。不改 observer/event/ToolCard bool wire。

## 4. ChannelDecider、agent-task 与 redirect budget

- [x] 4.1 **RED**：给 `PermissionRequest` 增 `permission_level` 与 gate 传入的 `network_preview`；生产构造先放错误占位。写 ChannelDecider 测试：有效 Network 在 Normal / AcceptEdits / Plan 发带同一 preview 的请求、Yolo 不发 channel；不可授权 preview 在含 Yolo 的所有 mode 跳过 policy / auto Allow并发 reject-only 请求；异常 Allow 回复仍 Deny；sender / responder 断开 fail-safe。运行 targeted tests，确认 preview 传递 / Yolo reject-only 断言红。
- [x] 4.2 **GREEN**：最小接通 preview 与 request 字段，顺序固定为 unsupported preview → command policy → valid mode → oneshot；reject-only 只接受 Deny，keyless AllowAlways 不记忆 / 不落盘，保持 command 持久化失败 Notice。
- [x] 4.3 **integration regression**：用无终端 Mock provider + counting WebFetcher 证明 authorizable web_fetch / web_search 用户 Deny 得 `UserDenied`；未知 Network default preview、畸形 web args、Yolo 下不可授权调用无论 decider Deny / Allow均得 `NetworkUnauthorizable(原 reason)`，reason 进入 is_error history且不得写成 user denied；以上路径均零 execute / 零 fetch并进入下一轮。有效 AllowOnce 每 ToolCall 只过一次 gate，下一调用仍再问，DDG 结果 URL 不额外 fetch。
- [x] 4.4 **RED**：给纯 `redirect_allowed(redirects_followed)` 加 compile stub 与运行期失败测试：0 / 1 / 2 允许处理下一 redirect，3 拒绝第四次并映射 `too many redirects`；不使用真实网络。
- [x] 4.5 **GREEN**：最小实现 `redirects_followed < MAX_REDIRECTS` 并让 ReqwestFetcher 当前 redirect 判断点消费该 helper；除抽取判断外不改 URL join、HTTP、SSRF 或 transport，使 §4.4 全绿。
- [x] 4.6 **characterization / regression**：重跑既有 URL、逐跳 SSRF、redirect response、DDG decode / parse tests；明确 redirect 上限由 §4.4 纯测试证明，transport 与 SSRF 由既有回归证明，不新增 transport seam / live network。

## 5. Terminal-safe formatter、共享 layout 与 TUI barrier

- [x] 5.1 **RED**：先为 presentation-neutral preview formatter 写失败测试，只输入结构化 preview、不接收 tool name；覆盖 lossless full args、canonical target / scope、literal backslash，以及 `char::is_control()`、`Bidi_Control` / `Default_Ignorable_Code_Point`、combining / variation 代表值（至少 U+009B、U+202E、U+2066/U+2069、U+200B）。default / invalid preview 只产 reject-only generic args + reason。仅加空字符串 compile stub，运行 targeted tests 得断言红。
- [x] 5.2 **GREEN**：在新建 `src/permission/preview.rs` 实现 JSON serialization + 本地 Unicode range table → 可逆 ASCII `\u{HEX}`，并在 `src/permission/mod.rs` 导出，使 §5.1 全绿；不得按 web 工具名构造 target / scope，不新增 crate。
- [x] 5.3 **RED**：新建 `src/tui/permission.rs`，为纯 `network_permission_layout(area, preview, scroll) -> NetworkPermissionLayout` 写失败测试；结果锁 wrapped lines、visible range、clamped scroll、total lines、位置提示与 `can_allow`。覆盖正常 / 极窄 / 高度不足、宽字符、首尾翻页及 Resize；固定 scope / 动作任一裁剪时必须 false。仅加默认 compile stub，运行得断言红。
- [x] 5.4 **GREEN**：在 `src/tui/permission.rs` 最小实现共享 layout 与 scroll clamp，并由 `src/tui/mod.rs` 声明 module，使 §5.3 全绿；render 和 event approval path 后续必须调用这一函数，禁止复制 geometry 或信任旧 frame cache。
- [x] 5.5 **RED**：为 request-generation / input barrier 的纯状态转换写失败测试：新请求 scroll=0、armed=None；同 generation 成功绘制并隔离旧 Key/Paste 后才 armed；旧 generation / barrier 前 Allow 无效且不延迟；Resize unarm；重新绘制 / barrier 后恢复；Deny 始终可用；responder 至多完成一次。
- [x] 5.6 **GREEN**：最小实现 generation、armed 与 barrier 状态转换，使 §5.5 全绿；这些字段保持进程内，不进 session。
- [x] 5.7 按 `设计规范/03-组件清单.md` C6 与 `设计规范/02-布局与交互.md` 接入 frame：只渲染 PermissionRequest 携带的 preview；有效态显示完整参数 / canonical target / preview scope与 y/n，reject-only 显示 generic args / 原因且仅 Deny；复用 warning token / box / pending 位置，不新增 theme token。
- [x] 5.8 **post-wiring regression**：让 `render_permission` 与 `process_event_batch` 消费同一 layout；approval handler 用当前 terminal area 重算。以真实 event batch + oneshot 锁：预缓冲 y/Enter/Paste 不授权；barrier 后新 y 才允许；初始小尺寸、Resize 缩小 / 恢复与旧 generation；四滚动键不改 transcript / history；reject-only / `a` 不允许；n/Esc 始终 Deny。
- [x] 5.9 生成五份 `.snap.new`：Midnight authorizable web_fetch 首屏 + 末屏、Midnight reject-only、Midnight small-terminal fail-closed、Daylight web_search。覆盖 terminal-safe escape、scroll、canonical target、preview scope、generation ready / 不可允许动作和输入框 / 状态行可见；small-terminal 快照须显示不可检查 / 仅拒绝状态，此步不 accept。

## 6. Headless CLI Network prompt（强制 TDD）

- [x] 6.1 **RED**：在 `src/cli.rs` 为纯 CLI formatter / 可注入 writer-reader 薄壳写失败测试：有效 preview 的完整 prompt 严格按 format → write_all → flush → read；short-chunk writer 最终完整写入后可读；fail-after-N Err、WriteZero、flush error均 Deny且 reader=0；不可授权 preview 输出 generic args / reason 后 Deny且 reader=0；y/yes Allow，其他 / EOF / read failure Deny；Edit / Execute prompt 零回归。仅加 compile stub，运行得断言红。
- [x] 6.2 **GREEN**：让 StdinDecider 消费 gate 传入 preview并最小实现 fail-closed I/O；完整 flush 后才 `spawn_blocking` 读 stdin，不可授权或输出失败不读，不增加 always-allow / CLI flag，使 §6.1 全绿。
- [x] 6.3 **integration regression**：用内存 reader/writer + counting WebFetcher 组合证明有效 prompt 顺序、short writes 最终完整、长参数不截断，以及 unknown / invalid / n / EOF / read / fail-after-N / WriteZero / flush failure 均零 execute / 零 fetch；不得依赖真实 stdin、终端或网络。

## 7. 文档、兼容与 archive readiness

- [x] 7.1 更新 README 权限模型、TUI / headless web 工具说明与 CHANGELOG `[Unreleased]`：有效 Network 默认逐次询问、Yolo 仅自动放行 authorizable preview、未知 / 畸形始终拒绝、SSRF 始终强制、Provider transport 不在 Tool gate 内；不预写 archive 后数量。
- [x] 7.2 只读核对 6 个 delta spec 与 design 的非 checkbox Archive Checklist 已完整编码 sync handoff：四 level、四 mode、builtin 分类、CLI fail-closed、TUI 四模式 / barrier 与旧 requirement rename 清理。将摘要写入实施报告；本 task 不执行 sync / archive move、不更新 README archive 数、不写 `.ai_history`。
- [x] 7.3 确认 `Cargo.toml` / `Cargo.lock` 无新依赖、CLI flags / stdin grammar 不变、config 无 `allowed_network_*`、session fixtures / wire 无 generation / layout / preview 字段；默认生产 registry 只装配 `ReqwestFetcher`，任何新增生产 fetcher 均有 scope conformance tests，`MockFetcher` 仅在测试使用且零网络；明确后续项仍为 per-origin allowlist、origin-scoped reauth、DLP、Network card effect 持久化。

## 8. 用户视觉停点与快照接受

- [x] 8.1 **用户专属人工视觉停点**：由用户对 `设计规范/` 与原型截图审阅 §5.9 五份 `.snap.new` 的 port / adapt / drop、有效 / reject-only / small-terminal fail-closed、长参数首尾与 Daylight；主 agent / 自动实施 agent 均不得代勾。
- [x] 8.2 用户明确批准后，仅 accept §5.9 五份新快照；不得顺手 accept 其他 `.snap.new`。
- [x] 8.3 复核既有 short diff、daylight、allow-always、long-diff 权限框快照零 churn；Network ToolCard legacy `readonly=false` 不显示“只读 · 自动运行”，session JSONL 零字段变化。

## 9. 自动化门禁

- [x] 9.1 targeted：preview invariant / canonical / current-fetcher scope、`PermissionCheck` Some / None、`PermissionGateOutcome` 归因 / reason history、gate clamp / Yolo、生产 registry / fetcher scope conformance、schema / Plan instruction、ChannelDecider、counting WebFetcher、redirect budget、terminal-safe formatter、layout / barrier / TUI wiring、CLI I/O tests 全绿，且无 `.snap.new` 残留。
- [x] 9.2 全量：运行 `cargo fmt --all -- --check`；随后在每个独立 PowerShell shell 中设置 `$env:CARGO_TARGET_DIR='target/codex-network-permission'`，依次运行 `cargo clippy --all-targets --locked -- -D warnings`、`cargo test --locked`、`cargo build --release --locked`。不得 kill 用户进程，不得回退默认 target 规避失败。
- [x] 9.3 OpenSpec：`openspec validate add-network-permission-level --strict` 与 `openspec validate --all --strict` 均通过；`openspec status --change add-network-permission-level` 为 4/4，`openspec instructions apply --change add-network-permission-level --json` remaining 与未勾 checkbox 一致（用户专属 8.1 / 10.x 完成前 state 可为 ready）。
- [x] 9.4 范围复核：`git diff --name-only` / `git diff --check`、未跟踪文件、snapshot / dependency diff 均符合本 change；不得用 `cargo fmt` 制造无关 churn。

## 10. 真机核验（用户专属；主 agent / 自动实施 agent 均不得代勾）

- [x] 10.1 Normal：长 web_search / web_fetch C6 可滚到最后字符，canonical target / scope 正确；请求前预输入 y/Enter 不授权，barrier 后新 y 才允许；小终端 / Resize fail-closed，n/Esc 始终可用；AllowOnce 后下一同目标仍再问。
- [x] 10.2 AcceptEdits / Plan：authorizable Network 均询问；Plan 可见 web、指令保留每步 validation，Deny 不退出 Plan，Edit / Execute 不可见 / 被纵深拒。
- [x] 10.3 Yolo：合法 web preview 不弹 C6直接执行；未知 Network / 畸形 web args 仍 reject-only / Deny且零网络；loopback / 私网 / link-local 仍由 SSRF blocked。
- [x] 10.4 `--headless`：有效 web prompt 完整显示 terminal-safe args / canonical target / scope，n / EOF 零网络、y 只放行本次；未知 / 畸形 preview 输出原因后不读 stdin；Edit / Execute prompt 不变。
- [x] 10.5 既有权限回归：ReadOnly 自动运行；AcceptEdits 只自动 Edit；Execute command allowlist / `[a · 总是允许]`、持久化失败 Notice、Shift+Tab pending 切换均保持现状。

## 11. Archive checklist（不计入 apply progress；不得改为 checkbox）

仅在全部 checkbox 完成、用户完成真机核验并明确发起 archive 后执行：

1. 确认 artifacts 4/4 complete，`openspec instructions apply --change add-network-permission-level --json` 为 remaining=0。
2. 按 archive workflow 展示 delta sync 摘要并取得用户选择。
3. sync 后实际核对 / 更新主 specs：tool-system 四 level + preview；permission-gate Network clamp + 四 mode；builtin-tools 4 local ReadOnly + 2 Network + 3 mutation + 3 interactive；CLI output fail-closed；TUI 旧“三模式”与 web ReadOnly requirement 名均不残留。
4. 执行 archive move；随后按实际 archive 目录数量更新 README，不提前写预测值。
5. 按 AGENTS.md 起草本 change 的 `.ai_history/logs/...` archive 决策记录，交用户审阅，并与 archive 变更进入同一提交。
6. 运行 `openspec validate --all --strict`、范围 diff 与 archive 路径复核。
