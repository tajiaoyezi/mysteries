# 2026-07-18 · 64 · archive-add-agent-execution-scope

## 决策
- v1.3先建立通用Agent execution scope，再实现subagent | 选:稳定run identity、parent→child cancellation、iteration/deadline/depth预算与capability单调收窄 | 弃:直接堆叠`delegate_task`（生命周期与权限边界不完整）、引入第三方Agent SDK（违反核心能力自研边界） | 主导:讨论收敛 | 依据:code/tests/spec
- 以Assistant提交点区分中断history语义 | 选:Provider/context返回Assistant前回滚未提交的当前User model history但保留TUI transcript；Assistant提交后保留User/Assistant并按occurrence补synthetic ToolResult | 弃:继续由TUI suffix helper猜测（headless/child不可复用）、保留未回答User进入下一轮（真机复现旧任务污染） | 主导:用户真机验证与讨论收敛 | 依据:RED→GREEN tests/manual verification
- capability采用受限registry与scope clamp双层防线 | 选:`ToolRegistry`内部共享`Arc<dyn Tool>`，schema、dispatch、scoped gate逐层取交集且先于ReadOnly/Yolo/allowlist | 弃:只隐藏schema（模型硬发可绕过）、为child重新构造工具（有状态实例分裂） | 主导:讨论收敛 | 依据:permission/tool tests/spec
- TUI Interrupt只发cancel并等待Agent内核收口 | 选:每个Prompt独立root scope、唯一terminal event、旧session normalization仅保留兼容用途 | 弃:直接drop run并由调用方补history、宣称硬终止blocking IO或外部进程 | 主导:讨论收敛 | 依据:TUI integration tests/snapshots/manual verification

## 变更
- 新增`agent-execution-scope`能力与scoped Agent入口，接通context、Provider、permission、串行/并行工具及forced-final termination。
- `ToolRegistry`迁为共享工具实例的受限视图；permission gate新增独立`ScopeViolation`，既有公开variant与legacy入口保持兼容。
- TUI中断改为cancel pinned scoped run并等待内核收口；session wire、布局、Interrupted文案与Midnight/Daylight快照零变化。
- 真机8.2首次发现Provider中断后旧Prompt污染下一轮；新增两层RED回归，修复未提交User turn回滚后复测通过。
- 自动化门禁通过：958个lib test、8个e2e、4 ignored；fmt、clippy、release build、RustSec与OpenSpec strict通过；真机8.1–8.5全部通过，tasks为45/45。
- 主spec同步新增14项requirement：`agent-execution-scope`5项、`agent-loop`4项、`permission-gate`2项、`tool-system`3项。

## 待决
- 无。`delegate_task`、只读subagent、child scheduler与child session留给后续独立change。

## 引用
- OpenSpec change: add-agent-execution-scope
- Specs: agent-execution-scope、agent-loop、permission-gate、tool-system
- Manual verification: add-agent-execution-scope/manual-verification.md
- Session checkpoint: 本次未单独生成
