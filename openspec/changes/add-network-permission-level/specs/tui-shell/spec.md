## MODIFIED Requirements

### Requirement: ChannelDecider 权限 oneshot 往返

`ChannelDecider` SHALL impl async `PermissionDecider`。`decide` MUST 按以下顺序处理:

1. 若 gate 传入 Network preview 且 `authorizable=false`,跳过 policy / mode Allow，创建 reject-only oneshot 请求；
2. 查注入的 command-only `PolicyEngine`;仅 Execute 的 permission key 可命中,Network MUST 不命中；
3. 对有效调用查 `auto_allows(mode, level)`;Yolo 的 authorizable Network / Edit / Execute 或 AcceptEdits 的 Edit 命中时直接 Allow,不发 channel；
4. 未命中时创建 oneshot,向 UI 发 `AgentEvent::PermissionRequired{tool_name, args, permission_level, network_preview, allow_always_key, responder}`,在 `rx.await` 挂起；
5. 有效请求的 `AllowOnce → Allow`;`AllowAlways →` 有 key 时记忆并持久化后 Allow、无 key(含 Network)时仅当前 Allow；reject-only 请求即使异常收到 Allow 也返回 Deny；`Deny`、channel sender / responder 断开 → Deny。

`PermissionRequest.permission_level` MUST 精确携带 Tool 声明的 level；Network 请求还 MUST 携 gate 预计算的同一 `NetworkPermissionPreview`，Channel / App / render 不得按 tool name 重建 target / scope。`allow_always_key` 仍只对现有命令 key 为 Some。`decide` 返回类型保持 `PermissionDecision {Allow, Deny}`。本机制不得把 UI 依赖放进 permission-gate 内核。

#### Scenario: 权限请求挂起-恢复

- **WHEN** `ChannelDecider::decide` 被调用(policy 与 auto_allows 均未命中),UI 收到 PermissionRequired 后经 responder 回送 AllowOnce
- **THEN** decide 返回 Allow(挂起在 rx.await、收到后恢复)

#### Scenario: UI 断开 fail-safe 拒绝

- **WHEN** 发送 PermissionRequired 失败,或发出后 UI responder 被丢弃
- **THEN** decide 返回 Deny,不 panic

#### Scenario: 命令 allowlist 命中不发起 channel 往返

- **WHEN** Execute 调用的 permission key 已在注入 PolicyEngine 中
- **THEN** decide 直接返回 Allow,不创建 oneshot、不发 PermissionRequired

#### Scenario: Network 在 Normal / AcceptEdits / Plan 发起请求

- **WHEN** 当前分别为 Normal、AcceptEdits、Plan,对一个 `authorizable=true` Network 工具调用 decide
- **THEN** 三者均发 PermissionRequired,其中 `permission_level=Network`、`allow_always_key=None`

#### Scenario: Network 在 Yolo 自动放行

- **WHEN** 当前为 Yolo,对一个 `authorizable=true` Network 工具调用 decide
- **THEN** 直接返回 Allow,不发 PermissionRequired

#### Scenario: 不可授权 Network 在 Yolo 仍发 reject-only 请求

- **WHEN** 当前 Yolo,gate 传入 `authorizable=false` 的 Network preview
- **THEN** 不命中 auto Allow；发出的 PermissionRequired 携同一 preview 且只能 Deny,最终 gate 亦 clamp Deny

#### Scenario: keyless AllowAlways 不持久化

- **WHEN** Network 请求异常回送 AllowAlways
- **THEN** 本次返回 Allow,但不调用 command append、不记忆 key,下一 Network 调用仍重新询问

### Requirement: agent-task 一轮编排(Mock 驱动 · 无终端)

系统 SHALL 提供可在无终端下以 Mock provider 驱动的 agent-task 编排:投入一个 prompt,经 `ChannelSink`(文本)与 `ChannelDecider`(权限)跑完一轮 `Agent.run`,把事件流回 channel。含非 `ReadOnly`(`Network` / `Edit` / `Execute`)工具的脚本 MUST 能走通「PermissionRequired → 回送决策 → 继续 / 拒绝入 history」。Network 测试 MUST 注入 mock / spy WebFetcher,不得依赖真实网络。

#### Scenario: 允许一个含权限的工具调用

- **WHEN** Mock 脚本为「轮1 → 一个提供有效专用 preview 的 Network / 一个 Edit / Execute 工具的 tool_call、轮2 → 终复文本」,投入 prompt 并对 PermissionRequired 回送 AllowOnce
- **THEN** channel 依次见到权限请求与文本事件,工具被执行,最终 TurnComplete;全程无终端

#### Scenario: 拒绝 Network 后零 fetch 且继续

- **WHEN** Mock 脚本轮1调用 Network 工具,UI 回送 Deny,工具持 counting WebFetcher
- **THEN** fetcher 调用数为 0,channel 收到 error 工具完成事件,history 含 denial ToolResult,轮2仍返回最终文本并 TurnComplete

### Requirement: 权限模式切换键与底部模式行

系统 SHALL 支持 `Shift+Tab`(`KeyCode::BackTab`)在 `Normal → AcceptEdits → Yolo → Plan → Normal` 间循环切换当前权限模式;切换 MUST 即时生效于后续工具决策(经共享模式句柄,与 agent-task 同一来源)。当前模式 SHALL 显示在状态行下方独立的底部模式行(屏幕最末行,不占状态行 C10),格式 `<glyph> <mode> · shift+tab 切换`;Normal / AcceptEdits / Yolo 沿用 `▸` / `▸▸` / `▲`，配色分别为 `text.muted` / `accent.primary` / `warning.fg`；Plan 沿用专属 `◔ plan` 指示与既有专属配色(详见「Plan 模式指示与 Shift+Tab 达 Plan」)。模式行 SHALL 常驻显示；切换键在任意 phase 可用(含 pending 权限框)。模式默认 Normal,不跨重启持久化。命中自动放行时不产生 pending C6；不可授权 Network 不受 mode 自动放行。

#### Scenario: Shift+Tab 四模式循环切换

- **WHEN** 当前 Normal,连续按 Shift+Tab
- **THEN** 依次切到 AcceptEdits、Yolo、Plan、Normal

#### Scenario: 底部模式行反映当前模式

- **WHEN** 切到 Yolo
- **THEN** 屏幕最末行渲染 `▲ yolo · shift+tab 切换`(`warning.fg`),且状态行 C10 不含模式段

#### Scenario: Yolo 下 Network / Edit / Execute 不弹权限框

- **WHEN** 当前 Yolo,模型分别调用 authorizable Network、Edit、Execute 工具
- **THEN** 三者均不产生 pending C6,直接进入工具执行；Network 仍须通过工具内部 SSRF 护栏；不可授权 Network 例外为 reject-only / Deny

#### Scenario: Plan 下 Network 弹框、Edit / Execute 不下发

- **WHEN** 当前 Plan,模型可见 Network schema 并调用 Network 工具
- **THEN** 产生 Network pending C6；Edit / Execute schema 不下发且越界调用由 agent-loop 纵深拒

## ADDED Requirements

### Requirement: Network 权限请求的 C6 呈现

当 `pending_permission.permission_level == Network` 时,TUI SHALL 只消费请求携带的 tool-owned preview，复用 `设计规范/03-组件清单.md` C6 与 `设计规范/02-布局与交互.md` 的位置、warning token、box-drawing与拒绝路径，并 adapt 为 Network 专用内容:

- 标题明确为“需要联网授权”并显示工具名；
- body 显示 terminal-safe、lossless escaped 的完整 args、preview 提供的 canonical initial target 与 scope；TUI MUST NOT 按 tool name 解析 / 重建 URL、DDG endpoint 或 redirect 次数；
- authorizable web preview 显示“允许本次调用；最多 N 次、可能跨站的公网重定向；每跳仍过 SSRF”，其中 N 直接取 preview；不得把 initial target 描述为 permission key；
- 参数按共享 `NetworkPermissionLayout` 的 display width 换行；超出 viewport 时显示行位置与 `↑↓/PgUp/PgDn 查看完整参数`；renderer 与 approval event path MUST 调同一纯 layout 函数，不得复制 geometry。content width 为 0、固定行裁剪或无法同时显示一行参数 / 位置 / scope / 动作时 `can_allow=false`；
- AppState 为每个请求分配 `request_generation`。新请求令 scroll=0并清空 armed；Resize 以新 viewport clamp scroll并清空 armed。同 generation 的 C6 成功绘制并隔离 frame 前已排队的 Key / Paste 后才 armed。`y` / `Enter` 仅在 generation 匹配、armed、preview authorizable 且当前 layout `can_allow=true` 时 AllowOnce；其他批准键忽略且不得延后生效。`n` / `Esc` 始终可 Deny；
- authorizable 且 `allow_always_key=None` 时动作行仅 `[y · 允许本次] [n · 拒绝]`；`a` 不响应。`authorizable=false` 时显示“无法验证网络目标，本次不可允许”与原因，只显示拒绝动作；异常 Allow 回复仍不得放行；
- 已知值与 generic JSON 均 MUST 先做无歧义 JSON serialization,再把 terminal-unsafe scalar 显式转成可逆 ASCII `\u{HEX}`；unsafe predicate MUST 覆盖 `char::is_control()`、Unicode `Bidi_Control` / `Default_Ignorable_Code_Point` 与 repo width 规则判为 0 的 combining / variation scalar（代表值 U+009B、U+202E、U+2066/U+2069、U+200B）,且 literal backslash MUST escape 后与真实 unsafe scalar 可区分；
- 未知 Network 工具、缺专用 preview、非法 args 或目标不可验证时 MUST 以 terminal-safe generic args + denial reason 呈 reject-only，不得虚构 target / redirect 上限，不 panic、不溢出布局。

此 UI 不新增布局区域；Network frame 仍钉在输入框上方。渲染 MUST 由 TestBackend + insta 的 Midnight / Daylight 带色快照验证；既有短 diff、allow-always、long-diff 权限框快照 MUST 零 churn。

#### Scenario: web_fetch Network 权限框快照

- **WHEN** pending 一个 `permission_level=Network`、authorizable preview 含 web_fetch 长 URL / canonical target / scope、allow_always_key=None 的请求并以 Midnight 渲染
- **THEN** 首屏快照含联网标题、URL 起始行、初始 origin、call-scoped 跨站 redirect 提示、scroll 位置与仅 y/n 动作,且输入框 / 状态行仍可见；滚到底部的快照可见 URL 最后一行且动作仍钉底

#### Scenario: web_search Daylight 权限框快照

- **WHEN** pending 一个 `permission_level=Network`、authorizable preview 含 web_search query / canonical DDG target / scope 的请求并以 Daylight 渲染
- **THEN** 快照含完整可达的 query、固定 DDG 初始目标、web_search 自身 redirect 授权说明、允许本次 / 拒绝动作,warning 语义配色来自 Daylight token

#### Scenario: 长参数可完整滚动且 scroll 不串请求

- **WHEN** Network 参数换行后超过 viewport,用户按 `↓` / `PageDown` 到末尾,随后完成当前决策并 pending 一个新请求
- **THEN** 当前请求的每一行与最后一个字符均可到达,动作行始终可见；新请求从第 1 行开始,旧 scroll 不残留

#### Scenario: 隐形与双向字符可见且无歧义

- **WHEN** URL/query 含 U+009B、U+202E、U+2066/U+2069、U+200B、combining mark 与 literal `\u{202E}`
- **THEN** terminal-unsafe scalar 均显示为可逆 ASCII escape,literal backslash 自身被 escape,字符不被终端执行、隐藏或重排,scroll 后全部表示均可达

#### Scenario: 终端过小不能盲批

- **WHEN** Network C6 可用高度不足以显示一行参数、位置提示与固定警告 / 动作,用户按 `y` 或 `Enter`
- **THEN** Midnight TestBackend 快照显示不可检查 / 仅拒绝状态；responder 不收到 AllowOnce,pending 保持并提示放大终端；`n` / `Esc` 仍可 Deny,放大到可检查后才恢复允许键

#### Scenario: renderer 与批准键共用当前 layout

- **WHEN** 初始小尺寸、Resize 缩小或 Resize 恢复后,分别渲染并处理 `y`
- **THEN** frame 与 event path 使用同一 `NetworkPermissionLayout`;不可完整呈现时两者均 `can_allow=false`,恢复尺寸并重绘 / rearm 后才同时为 true

#### Scenario: 预缓冲批准键不能跨 render barrier

- **WHEN** PermissionRequired 到达前已有排队 `y` / Enter / Paste,随后新 generation 的 C6 首次绘制
- **THEN** 旧输入被隔离且 responder 无值；只有 barrier 后新读到的批准键才可 AllowOnce,responder 至多完成一次

#### Scenario: Resize 令旧 armed generation 失效

- **WHEN** 已 armed 的 Network C6 收到 Resize,随后在重绘前按 `y`
- **THEN** `y` 无效；新尺寸成功绘制并完成新 barrier 后才恢复批准；`n` / Esc 始终可拒绝

#### Scenario: Network 不响应 a

- **WHEN** authorizable Network 权限框已 armed 且 allow_always_key=None,用户按 `a`
- **THEN** pending 保持、responder 未收到 AllowAlways；随后 y/Enter 或 n/Esc 仍正常完成一次决策

#### Scenario: 未知 Network 工具只可拒绝

- **WHEN** pending 一个未知 Network 工具且 args 缺 url/query 或字段类型错误
- **THEN** C6 以可滚动完整 generic args + denial reason 渲染,不显示 Allow；`y` / Enter 无效、`n` / Esc Deny,不 panic、不越过框高 / 宽度
