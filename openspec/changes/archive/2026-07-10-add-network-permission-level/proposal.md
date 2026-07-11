## Why

`web_fetch` / `web_search` 当前被归为 `ReadOnly`，因此在所有权限模式下都会绕过 `PermissionDecider` 静默出站；既有 SSRF 护栏只限制目标地址，不能阻止模型把工作区内容编码进公网 URL 或搜索词。现在需要把“本地只读”与“网络出站”拆成独立权限边界，在继续做并行工具、MCP 等扩展前先关闭已记录的 exfiltration 风险。

## What Changes

- **BREAKING（行为）**：`PermissionLevel` 新增 `Network`；`web_fetch` / `web_search` 从 `ReadOnly` 改为 `Network`，默认不再静默执行。
- 权限矩阵固定为：具有可验证、tool-owned preview 的 `Network` 调用在 `Normal` / `AcceptEdits` / `Plan` 均逐次询问、`Yolo` 自动放行；缺专用 preview、参数畸形或目标不可验证的 Network 调用在所有 mode 下 fail-closed。纯 `ReadOnly` 仍直接放行，`Edit` / `Execute` 既有语义不变。
- Plan 模式继续向模型下发 `Network` 工具 schema，允许 research-first 调研；agent-loop 的纵深拒只阻止 `Edit` / `Execute`，`Network` 必须进入权限门而不是被 schema-omit 或直接拒绝。
- Network 拒绝沿用既有零执行 / is_error history / 循环续跑链，但 gate outcome 区分用户 Deny 与系统 `NetworkUnauthorizable(reason)`，让模型可修正畸形参数；两者底层 `WebFetcher` 均零调用。即使用户允许或处于 `Yolo`，既有 SSRF / redirect 逐跳地址检查仍不可绕过。
- Network preview 改由 `Tool` 自身生成：`web_fetch` / `web_search` 的 preview 与 execute 共用 canonical request builder、DDG endpoint，并从当前所持 `WebFetcher::permission_scope()` 取得 redirect / SSRF 声明。生产 registry 仅装配策略与声明一致的 `ReqwestFetcher`；permission / TUI / CLI 层不得按工具名重建目标或 scope。generic args 只用于解释拒绝，永不构成可授权 fallback。
- TUI 复用 `设计规范/03-组件清单.md` 的 C6 权限框与 `设计规范/02-布局与交互.md` 的 pending 位置 / 键位：新增 Network 专用标题、可滚动完整参数、canonical 初始目标与 call-scoped redirect 范围。Allow 必须同时满足专用 preview 可授权、当前共享 layout 可完整检查、同 generation 的 C6 已成功呈现且输入 barrier 已 armed；旧批次 `y` / `Enter`、过小终端与不可验证 preview 均不能授权。
- `--headless` 的 `StdinDecider` 消费同一 preview：完整、terminal-safe 地 `write_all + flush` 后才读取 stdin；preview 不可授权或任一写 / flush 失败时直接 Deny 且不读 stdin。`Edit` / `Execute` 的既有 prompt 与 `y/yes` 解析保持不变，EOF 继续 fail-safe Deny。
- observer / 工具卡既有 `readonly: bool` 保持 wire 与 session 兼容，并继续精确表示“`PermissionLevel::ReadOnly` 且自动运行”；`Network` 记为 `readonly=false`，避免错误显示“只读 · 自动运行”。实际权限类型由 `PermissionRequest.permission_level` 单独携带，不改变持久化 `ToolCard` 结构。
- 明确不在本 change：per-host / per-origin 持久白名单、project config 网络授权、DLP / 参数自动脱敏、跨 origin redirect 二次弹框、并行工具、MCP。一次 Network 授权覆盖该次逻辑工具调用及其最多既有上限内的公网重定向；每跳仍过 SSRF 门。

## Capabilities

### New Capabilities

- 无；本 change 扩展既有工具、权限、编排、TUI 与 CLI runtime 能力，不新建 capability 域。

### Modified Capabilities

- `tool-system`：权限级别新增 `Network`、`Tool` 增 tool-owned preview 契约，并更新 Plan 模式 schema 过滤规则。
- `permission-gate`：以明确的 `PermissionCheck` 将 Network preview 传给 decider，对不可授权状态做最终 clamp，并区分 user denial / system unauthorizable outcome。
- `builtin-tools`：`web_fetch` / `web_search` 改为 `Network`，preview 与 execute 共用 canonical request truth，授权与 SSRF 职责分离。
- `agent-loop`：Plan 模式允许 Network 进入权限门，observer 的 `readonly` 语义与拒绝后的 history 行为明确化。
- `tui-shell`：ChannelDecider / C6 权限框承载 Network 请求并提供专用预览与逐次允许交互。
- `cli-runtime`：`StdinDecider` 承载 Network 请求时显示与 TUI 一致的完整参数和 redirect 授权边界。

## Impact

- **主要代码**：`src/tool/mod.rs`、`src/tool/web.rs`、`src/permission/mod.rs`、新增 `src/permission/preview.rs` 与 `src/tui/permission.rs`、`src/agent/mod.rs`、`src/cli.rs`、`src/tui/channel.rs`、`src/tui/app.rs`、`src/tui/mod.rs`、`src/tui/render.rs` 及其测试 / 快照。
- **UI / 设计规范**：复用 C6 的位置、warning token、box-drawing、`n` / `Esc` 与拒绝路径（port）；Network body、reject-only 状态、共享 `NetworkPermissionLayout`、request generation / input barrier 与滚动键优先级属于 adapt；浏览器式安全页、弹窗动画和新布局 drop。TUI 与 CLI 只格式化 Tool 提供的同一 preview，避免目标漂移与 control / bidi / zero-width 字符不可见。
- **配置 / 依赖**：不新增配置字段，不复用 `allowed_commands`，不新增 crate；现有命令白名单与 `AllowAlways` 行为保持不变。
- **兼容性**：无 CLI flag / stdin decision protocol / session 文件格式变化；headless Network prompt 的文案增强与网络工具默认询问是刻意的安全收紧。内部 `PermissionLevel` 的穷尽 match 会在编译期暴露所有待接线点，需更新 README / CHANGELOG。
- **后续依赖**：完成后才能安全规划 `parallelize-safe-tool-calls`；并行安全仍需独立 effect / concurrency 元数据，不能以 `PermissionLevel` 代替。
