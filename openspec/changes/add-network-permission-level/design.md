## Context

`PermissionLevel` 当前只有 `ReadOnly / Edit / Execute`。`gate` 对 `ReadOnly` 直接放行，`ChannelDecider` 只处理另外两级；`web_fetch` / `web_search` 因而在 `Normal`、`AcceptEdits`、`Yolo`、`Plan` 四种模式下都可静默出站。既有 SSRF change 已把地址检查收紧为初始 URL 与每跳 redirect 均 fail-closed，但它解决的是“能访问哪里”，不是“用户是否同意产生网络请求或披露 URL/query”。

本 change 跨 `tool-system`、`permission-gate`、`agent-loop`、两项 builtin web tool、TUI C6 权限框与 headless `StdinDecider`。它必须同时保持 Plan research 可用、命令 allowlist 语义不变、session wire 零变化，并让拒绝发生在任何 DNS / proxy / TCP / TLS / HTTP 活动之前。

## Goals / Non-Goals

**Goals:**

- 把工具诱发的公网出站从本地 `ReadOnly` 中拆为 `PermissionLevel::Network`。
- 固定四模式矩阵：`Normal` / `AcceptEdits` / `Plan` 逐次询问，`Yolo` 自动放行。
- 保持 Plan 模式可下发并调用 Network 工具，但每次调用仍经过 decider。
- Network 拒绝时零网络、零工具副作用，并把拒绝结果作为 `is_error` `ToolResult` 送回模型。
- 在 TUI C6 与 headless prompt 中清楚呈现完整 URL/query、初始目标与 call-scoped redirect 授权范围。
- 保持现有命令 allowlist、CLI flags / stdin decision protocol、config、ToolCard/session JSONL 与第三方依赖不变。

**Non-Goals:**

- per-host / per-origin 的 session 或持久白名单、撤销 UI、project config 网络授权。
- DLP、敏感信息识别 / 自动脱敏、请求内容审查或域名信誉判断。
- 为跨-origin redirect 增加嵌套授权回调；本轮授权粒度是完整 `ToolCall`。
- 改造 Provider 自身的模型 HTTP transport；`Network` 只约束经 `Tool` 发起的网络活动。
- 并行工具、MCP、通用 effect system、Network sandbox、防火墙或代理配置。
- 修复既有 DNS rebinding TOCTOU、6to4 / Teredo 等 SSRF 纵深残留。

## Decisions

### D1 · `Network` 是第四个权限级，不是 `ReadOnly` 的 flag

`PermissionLevel` 增加 `Network`，语义为“工具执行会产生外部网络活动”。本地文件读取 / 搜索仍是 `ReadOnly`；文件变更是 `Edit`；本地进程执行是 `Execute`。`ReadOnly` 是“无需 Tool permission gate 授权”的类别，不是通用副作用 taxonomy：`submit_plan`、`update_plan`、`ask_user` 仍可通过各自 seam 更新计划状态 / 发起用户交互并保持 `ReadOnly`。模型 Provider 的协议请求也不属于工具权限门。

选择 enum 新变体而不是给 web 工具名写特判，理由是权限分类仍由 `Tool` 自声明，`gate`、mode 策略与未来新 Network 工具不依赖名字。替代方案“`ReadOnly + requires_network()` 双维度”被弃：当前权限决策、schema filter 与纵深拒都以单一 level 为输入，双维会让同一调用在三处组合判定并增加不一致面；真正的通用 effect / concurrency taxonomy 留给后续并行工具 change，不能从本 enum 推断可并行性或所有进程内状态变化。

### D2 · 四模式矩阵固定，Network 只有 `Yolo` 自动放行

| `PermissionMode` | `ReadOnly` | `Network` | `Edit` | `Execute` |
|---|---|---|---|---|
| `Normal` | 直接放行 | 有效 preview 询问；不可授权则拒绝 | 询问 | 询问 |
| `AcceptEdits` | 直接放行 | 有效 preview 询问；不可授权则拒绝 | 自动放行 | 询问 |
| `Yolo` | 直接放行 | 有效 preview 自动放行；不可授权仍拒绝 | 自动放行 | 自动放行 |
| `Plan` | 直接放行 | 有效 preview 询问；不可授权则拒绝 | schema-omit + 纵深拒 | schema-omit + 纵深拒 |

`gate` 只对 `ReadOnly` 直接 `Allow`。它构造 `PermissionCheck{call, tool, network_preview}`：非 Network 的 preview 恒为 None，Network 为一次计算所得 Some。decider 仍只返回 Allow / Deny；gate 返回 `PermissionGateOutcome::{Allow, Deny(PermissionDenial)}`，其中 `PermissionDenial::{UserDenied, NetworkUnauthorizable(String)}`；下文以 `Deny(UserDenied)` / `Deny(NetworkUnauthorizable(reason))` 简写。Network 在 decider 返回后对 `authorizable=false` clamp 为 `NetworkUnauthorizable`；因此异常 decider、`AllowAlways` 与 `Yolo` 都不能绕过。有效 Network / Edit / Execute 才进入既有 mode；command policy 仍只认识 Execute + command。

替代方案“AcceptEdits 同时允许 Network”被弃：AcceptEdits 只代表本地编辑授权；把公网披露绑定进去会扩大既有模式承诺。替代方案“Plan 自动允许 Network”被弃：research 需要工具可见不等于同意静默出站。

### D3 · Plan 保留 Network schema，纵深拒只挡 `Edit / Execute`

`ToolRegistry::schemas_for(Plan)` 从 `ReadOnly || plan_only()` 改为 `ReadOnly || Network || plan_only()`，维持注册顺序。非 Plan 模式仍下发所有非 `plan_only` 工具。

agent-loop 的轮顶 mode 快照规则不变。Plan 纵深拒从“level != ReadOnly”收窄为“level 是 Edit 或 Execute”；Network 不被直接拒，必须进入 `gate`。Plan transient system instruction 保留既有三分支与验收契约并追加 Network 语义：“用户只是问 → 直接答；撞歧义 / 岔路 → `ask_user`；要执行任务 → `submit_plan`，且每一步带可独立验收的 `validation`；本地研究只读，web 调研可用但每次会请求 Network 授权；不得编辑或执行命令。”同批 `submit_plan + edit/execute` 仍由轮顶快照封住，不能借批准中途翻 mode 逃逸。

### D4 · `AllowOnce` 授权完整 ToolCall，而非 origin

Network permission 的授权对象是当前一个 `ToolCall`。`web_fetch` 与 `web_search` 均复用 `ReqwestFetcher`：一次允许覆盖初始 GET 以及该调用内部最多 `MAX_REDIRECTS = 3` 次 redirect（因此最多 4 次 HTTP GET），redirect 可跨 origin。`web_search` 的初始目标固定为 DDG endpoint；DDG HTML 中解析出的结果 URL 仅作为文本返回，不自动抓取，但 DDG 请求自身的 HTTP redirect 仍在本次授权内。相同 origin 的下一次 ToolCall 仍须重新授权。

初始 origin 只是 UI preview，不是 permission key 或信任边界。C6 必须明确显示“允许本次调用（含最多 3 次、可能跨站的公网重定向）”，不得写成“允许此 origin”。

选择 call-scoped 而不是跨-origin 二次询问，理由是 redirect 循环位于 `ReqwestFetcher` 内，而权限门位于 `tool.execute` 外；加入 per-hop authorizer 会把 UI/oneshot 重新耦进 web transport。替代方案“跨-origin fail-closed、让模型再发一次 web_fetch”更严格，但会改变大量正常 redirect 行为并扩大本轮兼容面。该取舍的残留风险是：获准的首个公网服务可决定后续最多 3 个公网目标；用户通过明确文案接受这一调用级委托。

### D5 · 权限与 SSRF 是串联而非互相替代

执行顺序固定为：

1. agent-loop 调 `gate`；
2. Deny → 不调用 `tool.execute`，spy `WebFetcher` 调用数为 0，history 收到 is_error；
3. Allow → `web_fetch` / `web_search` 调 `WebFetcher`；
4. `ReqwestFetcher` 对初始 URL 和每跳 redirect 继续执行 `precheck_url`、DNS resolve、`check_resolved` 后才发 HTTP。

“零网络”包括 DNS、proxy 连接、TCP、TLS 与 HTTP；本地 JSON / URL parse 和 preview 不算网络。用户允许或 `Yolo` 绝不覆盖 SSRF 拒绝。若公网 A 已获准并已请求，随后 redirect 到被 SSRF 拒绝的 B，只保证对 B 零 HTTP，A 的请求不可回滚。

HTTP GET 不被视为无副作用：DNS 会披露 hostname，远端可见 IP、时间、User-Agent、path/query，GET 也可能触发 tracking、webhook 或错误设计的状态变更。Network 表达用户对这些出站影响的同意，不表达远端可信。

### D6 · 首版只允许本次，不复用 command allowlist

Network 调用的 `PolicyEngine::permission_key` 恒为 `None`，因此 `PermissionRequest.allow_always_key` 为 `None`，C6 不显示 `[a · 总是允许]`；即使异常路径送回 `AllowAlways`，也只能退化为本次 `Allow`，不得记忆、落盘或影响下一调用。

不复用 `allowed_commands`：其 key 是命令空白规范化串，且 user / project config 当前按并集合并；若照搬为 host allowlist，恶意 workspace 可预授权 exfiltration origin。未来独立 change 若引 origin allowlist，必须只接受 user-owned 配置，以 `scheme + canonical host + effective port` 为 exact key，并重新设计 redirect 与撤销语义。

### D7 · Network preview 由 Tool 拥有，gate 使用同一 canonical truth

`Tool` 增加纯方法 `network_permission_preview(&Value) -> NetworkPermissionPreview`，default 返回 `authorizable=false`。结构至少包含 `authorizable`、原始完整 args、canonical initial target、scope（`max_redirects / may_cross_origin / ssrf_each_hop`）与不可授权原因。generic JSON 只能帮助解释拒绝，不能成为可授权 fallback。方法不得执行 DNS、HTTP 或 `WebFetcher`。

`gate` 对 Network 计算 preview 一次并把同一值传给 `PermissionDecider`；decider / TUI / CLI 不得按工具名重新构造目标、DDG endpoint 或 redirect scope。decider 返回后，gate 对 `authorizable=false` 再次 clamp 为 Deny。这样即使自定义 decider、Yolo 或异常 AllowAlways 返回 Allow，也不能执行未知工具、畸形参数或无法验证目标的 Network 调用。

`web_fetch` preview 与 execute 共用一个纯 request builder，builder 的 canonical `reqwest::Url` 同时成为 preview target 与实际交给 `WebFetcher` 的 URL。`web_search` preview 与 execute 共用 `ddg_search_url(query)`。`WebFetcher` 增 `permission_scope()`；preview 必须读取当前 tool 所持 fetcher 的同一 scope，fetcher 的 `fetch` 行为 MUST 遵守自己声明的 redirect / cross-origin / SSRF policy。生产 registry 只装配 `ReqwestFetcher`，其 scope 从 `MAX_REDIRECTS` 与逐跳 SSRF 实现生成；其他生产实现须通过同一 policy conformance tests。MockFetcher 仅为零网络 test double。formatter 不硬编码 `3`。canonical 用例锁定 userinfo、IDN/punycode、数字 IP、IPv6 与默认端口。

`src/permission/preview.rs` 的 presentation-neutral formatter 只格式化结构化 preview。展示值先经 lossless JSON serialization，再把 `char::is_control()`、Unicode `Bidi_Control` / `Default_Ignorable_Code_Point` 及 repo width 为 0 的 scalar 转成可逆 ASCII `\u{HEX}`；不新增 crate。literal `\u{...}` 与被 escape 字符必须可区分。

### D8 · TUI 使用共享 layout 与 request-generation input barrier

`PermissionRequest` additive 增加 `permission_level` 与 gate 预计算的 `network_preview`；AppState 为每个新 Network 请求分配进程内 `request_generation`，并维护 `permission_scroll` 与 `armed_generation`，均不持久化。新请求令 scroll=0、armed 清空；Resize 仅按新 viewport clamp scroll 并清空 armed，不把旧 generation 的批准状态带过重绘。

新增 `src/tui/permission.rs`，其中单一纯函数 `network_permission_layout(area, preview, scroll) -> NetworkPermissionLayout` 计算 wrapped lines、visible range、clamped scroll、位置提示与 `can_allow`。renderer 与 event approval path MUST 消费同一函数，禁止复制宽高公式或信任旧 frame cache。`can_allow` 仅在 preview authorizable、当前尺寸能同时显示至少一行参数、位置提示、完整 scope 与动作行时为 true；content width 为 0、固定行裁剪或宽字符无法容纳时均为 false。

Allow 还必须通过 input barrier：新 generation 的 C6 成功绘制后，先隔离 / 丢弃在该 frame 之前已排队的 Key / Paste approval 输入，之后才设置 `armed_generation=Some(current)`。`y` / `Enter` 只有 generation 匹配、已 armed 且当前 layout `can_allow=true` 时才回 AllowOnce；barrier 前、旧 generation、Resize 重绘前或不可授权状态的批准键一律忽略且不得延后生效。`n` / `Esc` 在未 armed、过小终端与 reject-only 状态始终可 Deny。Resize 必须重新 unarm，成功重绘并完成新 barrier 后才恢复批准。

参数超出 viewport 时显示 `第 X/Y 行 · ↑↓/PgUp/PgDn 查看完整参数`，动作与 scope 钉底。有效 preview 的动作行为 `[y · 允许本次] [n · 拒绝]`；不可授权 preview 显示 terminal-safe generic args 与原因，只提供拒绝。C6 port/adapt/drop：位置、warning token、box-drawing 与拒绝路径 port；完整参数视口、reject-only、共享 layout、generation / barrier adapt；浏览器式安全页、弹窗动画和新布局 drop。

`AgentObserver` / `AgentEvent::ToolCallStarted` / 持久化 `ToolCard.readonly` 保持 bool wire；该 bool 继续严格等价于 `level == ReadOnly`。因此 Network 为 false，不显示错误的“只读 · 自动运行”徽章。替代方案“把完整 PermissionLevel additive 写入 ToolCard/session”信息更完整，但不影响授权正确性，会扩大 session 兼容、快照与历史夹具范围；留给后续统一 effect / concurrency 模型。

### D9 · headless `StdinDecider` 输出失败即拒绝

`StdinDecider` 直接消费 gate 传入的 preview。`authorizable=false` 时完整输出 terminal-safe args 与拒绝原因后立即 Deny，不读取 stdin。有效 preview 先生成完整 bytes，再以可注入 writer 执行 `write_all` 与 `flush`；合法 short write 由 `write_all` 继续直至完整成功。serialization / format error、`write_all` 在部分进度后返回 `Err` / `WriteZero`、或 flush error 才直接 Deny，reader 调用次数必须为 0。仅在完整 prompt 成功 flush 后，才经 `spawn_blocking` 读取 stdin；`y/yes` Allow，其他输入、空行、EOF 与 read failure Deny。

不把 Network 文案只写在 `StdinDecider` 内部：CLI formatter 是纯函数，直接消费共享 preview，锁定两前端的 args 与 scope 不漂移。CLI flags、stdout 模型流、stdin decision grammar 均不变。

### D10 · 测试遵循内核 TDD、TUI 事后快照

`PermissionLevel`、tool-owned preview、gate clamp、mode matrix、Plan schema / instruction、redirect budget、terminal-safe formatter、共享 layout / scroll 与 CLI I/O 均是 headless 纯逻辑 / Mock 可驱动路径，实施时严格 RED→GREEN，并在新权限路径首次成型后停点确认。Plan instruction 测试必须直接检查实际注入文本的 Network、禁止 Edit/Execute 与每步 `validation` 语义，不得用 `PLAN_MODE_INSTRUCTION` 常量自身作 expected。

TUI 外壳补真实 event batch + oneshot 回归：预缓冲批准键、render barrier、旧 generation、Resize unarm / rearm、共享 geometry 与 responder 单次完成。之后再做 `TestBackend + insta`：Network 有效首屏 / 末屏、reject-only、小终端及 Daylight 对照；既有短 diff、allow-always、long-diff 快照必须零 churn。

现有 redirect / SSRF transport 不重写。抽取纯 `redirect_allowed(redirects_followed)` 供 loop 使用并锁定 0/1/2 可跟随、3 拒绝第四跳；其余 URL join、HTTP、逐跳 SSRF 由既有回归证明。不新增 live network 测试、transport seam 或 crate。

## Risks / Trade-offs

- **[提示疲劳]** 每次 DDG 搜索与 web_fetch 都询问 → 以安全默认换取显式同意；用户可显式切 `Yolo`，per-origin allowlist 另开 change。
- **[Yolo 可静默出站]** prompt injection 在 Yolo 下仍可能发公网请求 → Yolo 本就是全自动高风险模式；模式行持续可见，SSRF 仍强制。
- **[call-scoped redirect 边界较宽]** 首站可选择后续公网目标 → C6 明示跨站 redirect 范围，限制既有 3 次上限并保持逐跳 SSRF；origin-scoped reauth 另议。
- **[无 DLP]** 用户允许后仍可能把本地内容放进 URL/query → 明确属于本 change Non-Goal；TUI 以 terminal-safe escape + scroll、CLI 以完整 stderr 输出让实际 args 在批准前可检查，仍由用户判断。
- **[输入抢跑]** 请求出现前缓冲的批准键可能误授权 → request generation + successful-render barrier + stale Key/Paste 隔离；Resize 强制重新 armed。
- **[preview 漂移 / 缺失]** 前端重建目标、新 Network 工具忘记专用 preview，或 fetcher 声明与行为不符 → Tool-owned canonical preview、当前 fetcher scope、生产装配封闭 + conformance tests、gate 单次计算与最终 Deny clamp；default 永不可授权。
- **[Network 工具卡无专属 badge]** session wire 零变化的代价是执行后卡片只凭工具名体现联网语义 → pending C6 是授权事实源；未来 effect 模型统一解决，不在本轮加 shadow 字段。
- **[现有 spec 漂移]** `tui-shell` 同时存在三模式与四模式文字 → 本 change 完整替换被触及的旧 requirement，使其与 code / 后续 spec 对齐，不静默保留冲突。

## Migration Plan

1. 先以 TDD 扩 `PermissionLevel`、`gate`、`auto_allows` 与 Plan schema filter；编译器驱动补齐穷尽 match。
2. 将两个 web 工具改为 Network，更新 agent-loop Plan 纵深拒和 system instruction。
3. 实现 Tool-owned canonical preview、gate preview 传递 / clamp 与纯 redirect budget；两个 web 工具共用 request truth。
4. 接通 ChannelDecider、共享 layout、generation / input barrier 与 scroll；完成事件回归和用户视觉确认后 accept 快照。
5. 让 `StdinDecider` 消费同一 preview，以 fail-closed writer seam 补 headless 回归。
6. 更新 README / CHANGELOG；在隔离 `CARGO_TARGET_DIR=target/codex-network-permission` 下运行全量门禁，不 kill 用户进程。
7. 发布后有效 Network 默认逐次询问，未知 / 畸形 Network 始终拒绝；无 config / session 数据迁移。

回滚只需恢复旧代码与 specs；无数据迁移需要逆转，但回滚会重新打开 silent outbound 风险，release note 必须明确。

## Archive Checklist（不计入 apply progress）

仅在全部 active checkbox 完成、用户完成真机核验并明确发起 archive 后执行：

1. 确认 artifacts 4/4 complete、apply remaining=0；展示 delta sync 摘要并取得用户选择。
2. sync 后实际更新主 Purpose：`tool-system` 四 level；`permission-gate` Network gate + 四 mode；`builtin-tools` 4 local ReadOnly + 2 Network + 3 mutation + 3 interactive；并确认旧 web `ReadOnly` requirement 名与 TUI 三模式冲突不再残留。
3. 执行 archive move 后，按实际 archive 目录数量更新 README，不提前写预测值。
4. 按 AGENTS.md 起草本 change 的 archive 决策记录，交用户审阅，并与 archive 变更进入同一提交。
5. 运行 `openspec validate --all --strict`、范围 diff 与 archive 路径复核。

## Open Questions

无。per-origin 持久授权、origin-scoped redirect reauth 与 Network 工具卡完整 effect 持久化均已明确推迟到独立 change，不阻塞本轮。
