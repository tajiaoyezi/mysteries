# 2026-07-10 · 58 · archive add-network-permission-level

## 决策
- 工具出站网络独立为 `PermissionLevel::Network` | 选:第四权限级 | 弃:`ReadOnly` flag 与按工具名特判(判定分散、易漏接) | 主导:用户发起 + 讨论收敛 | 依据:安全审查、code、tests
- 四模式矩阵固定为 Normal / AcceptEdits / Plan 逐次询问、Yolo 仅自动允许可授权 Network | 不可授权 preview 在所有 mode 下 fail-closed | 弃:AcceptEdits 或 Plan 静默联网 | 依据:权限边界与真机核验
- 首版授权粒度为当前 ToolCall | 允许既有 redirect budget 内可能跨 origin 的公网跳转,每跳仍强制 SSRF | 弃:直接复用 command allowlist、首版引 per-origin 持久授权或 transport 内嵌套弹框 | 主导:讨论收敛 | 依据:现有 gate / WebFetcher 分层
- Network preview 归 Tool 所有并与 execute 共用 canonical request truth | gate 只计算一次并在返回点最终 clamp | 弃:TUI / CLI 按工具名重建 URL、DDG endpoint 或 scope | 依据:conformance tests
- TUI 授权必须通过共享 layout、当前 terminal geometry、request generation 与 render/input barrier | headless 必须完整 write_all + flush 后才读 stdin | 弃:信任旧 frame、预缓冲批准键、输出失败后继续询问 | 依据:事件回归、快照与用户真机测试
- 保持 CLI flags、stdin decision grammar、config、ToolCard/session wire 与第三方依赖不变 | Network 首版只允许本次 | 依据:兼容性审查

## 变更
- 新增 Network 权限级、结构化 preview/scope、gate 拒绝归因与 Plan research 路径。
- `web_fetch` / `web_search` 改为 Network,并让 preview、执行 URL、fetcher scope 与 redirect/SSRF policy 同源。
- TUI 新增 terminal-safe Network C6、滚动、reject-only、小终端 fail-closed 与输入 barrier;headless 新增完整 prompt 和失败即拒绝的 I/O seam。
- 接受五份 Network 快照,补齐内核 TDD、TUI 事件回归、CLI I/O、redirect budget、README 与 CHANGELOG。

## 待决
- `parallelize-safe-tool-calls`:独立 effect/concurrency metadata,不能从 PermissionLevel 推断并行安全。
- per-origin 持久授权、origin-scoped redirect reauth、DLP 与 Network 工具卡完整 effect 持久化分别留给后续 change。

## 引用
- OpenSpec change:`add-network-permission-level`
- 关联决策:`add-web-tools`(log 52)、`add-web-ssrf-guard`(log 53)、`add-plan-mode`(log 54)、`add-plan-persistence`(log 57)
- 本次 session:apply → code review → 修复 → 快照审阅 → 10.1–10.5 真机核验 → merge
