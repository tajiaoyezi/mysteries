## Why

`add-agent-execution-scope` 已把一次 Agent run 固化为可派生、可取消且不能扩权的执行单元，但产品仍不能把独立的只读调研任务交给 child Agent。现在需要用最小、可审计的 `delegate_task` MVP 消费这套基础，验证 subagent 的生命周期、权限与结果回传闭环，再讨论可写 child、后台任务或更深层 Agent graph。

## What Changes

- 新增内置 `delegate_task` 工具：parent 可把一个边界清晰的调研任务交给临时 child Agent，并把 child 的最终文本作为普通 `ToolOutcome` 返回当前 Agent Loop。
- child 在有效调用开始时原子取得当前 Provider / model snapshot，并在该次 invocation 全程固定使用；它复用调用时工作目录与既有 Agent Loop，但使用独立的 system prompt、history、iteration/deadline预算和只读 `ToolRegistry`。
- child 只暴露 `list_dir` / `read_file` / `glob` / `grep`；不暴露 Network、Edit、Execute、Plan、用户交互或 `delegate_task`，并以 execution scope capability 作为不可绕过的第二层限制。
- child 的四个读取工具额外受 canonical workspace root约束；绝对路径、`..`、symlink或junction解析后越出root均 fail-closed。该边界也覆盖目录walker为匹配规则读取的parent、`.ignore`与`.gitignore`控制文件：workspace外规则不得被读取或影响结果，workspace内规则文件canonical后越界必须fail-closed。普通root Agent既有路径行为不变，system prompt只作defense-in-depth而不是安全边界。
- 产品 root run 显式允许一层 child depth；`delegate_task`在depth不足的scope中同时从schema与dispatch隐藏/拒绝。legacy `Agent::run` / `run_observed` 兼容入口继续保持 depth 0，避免下游调用方在未选择新能力时看到或意外派生 child。
- 为工具执行增加可取得当前 execution scope、observer与child-only读取边界的 source-compatible seam；既有工具继续使用原 `execute` 行为，`delegate_task` 用它派生child scope，四个读取工具只在存在child边界时增加containment检查。
- 连续 `delegate_task` 调用可按 `ParallelSafe` 与相邻的其他eligible只读工具进入既有有界批次；固定上限 4 只限制同时active的outer tool/child invocation，第5项及后续occurrence等待空位后仍会执行，不构成每轮调用总量上限。物理完成可乱序，parent history / observer / Provider 可见的结果顺序仍按模型 occurrence。
- parent cancellation或deadline传播到所有in-flight child并由outer Agent Loop生成既有synthetic termination；outer Loop在tool outcome ready后、发布前再次裁决parent termination，防止child cancellation被误发为普通工具错误。仅child自身的成功、失败或独立deadline收口为大小受限且不泄漏内部history的稳定`ToolOutcome`。
- TUI 继续复用现有 C5 工具卡，只展示 parent 的 `delegate_task` 调用和最终结果；不新增 subagent 面板、child 工具卡或 session wire。该行为属于对现有组件的 **adapt**，不引入新的视觉 token或布局。
- 明确不提供递归 subagent、child session持久化、token总预算、后台/可恢复任务、Network/Edit/Execute child、MCP或第三方 Agent SDK。

## Capabilities

### New Capabilities

- `readonly-subagent`: 定义 `delegate_task` 的只读 child 装配、固定预算、单层派生、并发上限、取消传播、独立上下文和结果回传契约。

### Modified Capabilities

- `agent-execution-scope`: 产品 root run 显式获得一层 child-depth预算，同时保持 legacy wrapper 的 depth 0 兼容边界。
- `agent-loop`: 工具 dispatch 向 source-compatible scoped execution seam传递当前 scope，并保持并行委派的 occurrence顺序、取消收口与 observer 契约。
- `tool-system`: `Tool` 支持默认转发到既有 `execute` 的 scoped执行入口，使特定工具可安全消费当前 execution scope、observer与可选读取边界而不破坏已有实现。
- `builtin-tools`: 注册 `delegate_task`，锁定其 schema、`ReadOnly + ParallelSafe` 分类、只读 child 工具集合、child-only workspace containment及稳定输出/错误行为。
- `tui-shell`: root turn启用一层委派并复用现有 C5 工具卡；child内部事件不进入 transcript或 session，Interrupt仍只产生一个终态。

## Impact

- 主要影响 `src/agent/`、`src/tool/mod.rs`、新增的 subagent/delegation模块、`src/app.rs`、headless CLI与TUI root scope接线，以及相应 Mock / headless / TUI集成测试。
- Provider与model的动态切换必须由parent和未来child共享同一 runtime snapshot来源；provider+model成对切换必须经单次原子replace提交，单model切换保持单字段更新语义，不得让 `delegate_task` 永久捕获 assemble时的旧 Provider / model或观察到成对切换的中间组合。
- workspace containment必须使用canonical path判断并覆盖Windows symlink/junction及walker读取的ignore控制文件；parent ignore discovery必须止于canonical read root，不得仅做字符串前缀比较，也不得改变root Agent当前允许绝对路径与gitignore感知的兼容行为。
- 不新增dependency，不改变配置格式、CLI grammar、Provider wire、权限模式矩阵、session JSONL或现有视觉快照。
- 本 change 属于 headless Agent Loop、新工具与 execution scope路径，实施强制 RED→GREEN；允许先落只含类型、default与签名的可编译scaffold，但Agent dispatch消费scoped context及`delegate_task`真实行为都必须先RED。首次行为RED按仓库约定展示原始失败并等待用户批准；TUI仅做事后集成与零视觉churn验证。
