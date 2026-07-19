# Tasks — add-agent-execution-scope

执行边界：本 change 只建立可标识、可取消、有预算、capability 单调收窄的 Agent run 基础；不实现 `delegate_task` / subagent、child scheduler、MCP、token 总预算、session wire或新TUI组件。headless Agent Loop、ToolRegistry与新权限路径强制RED→GREEN；新接口与新权限路径的首次RED必须展示原始失败并等待用户确认。TUI仅替换当前turn的cancellation接线，视觉与快照必须零churn。Windows验证统一使用隔离的`CARGO_TARGET_DIR=target/codex-agent-scope`，不得kill用户进程或回退默认target规避锁。

## 1. Baseline、dependency与可编译接口scaffold

- [x] 1.1 **baseline**：在无实现改动时运行`$env:CARGO_TARGET_DIR='target/codex-agent-scope'; cargo test --lib --locked`，记录通过/忽略数；运行`cargo tree -i tokio-util --locked`记录当前`tokio-util 0.7.18`传递来源，并用`git diff --name-only`确认只含本change规划文件。
- [x] 1.2 **dependency**：在`Cargo.toml`直接声明`tokio-util = { version = "0.7", features = ["rt"] }`，用Cargo更新lockfile；审查`Cargo.lock`只给根`mysteries`依赖表增加direct edge、没有crate/version解析漂移。若出现无关升级，停止并报告。
- [x] 1.3 **scope scaffold（不写行为测试）**：新增`src/agent/scope.rs`（或等价模块），只落可编译的`RunIdentity`、`ExecutionBudget`、`ExecutionCapabilities`、`AgentExecutionScope`、派生错误与termination reason签名；不得`todo!()`/panic，不宣称目标行为完成。
- [x] 1.4 **Agent/observer scaffold（不写行为测试）**：新增独立`ScopedAgentError::{Agent,Cancelled,DeadlineExceeded}`、`run_scoped/run_observed_scoped`签名及带default适配的`on_scoped_*` observer方法；不得给既有`AgentError`增variant或修改legacy callback签名。机械恢复`NoopObserver`、`RecordingObserver`、`ChannelObserver`编译并运行`cargo test --lib --no-run --locked`。
- [x] 1.5 **registry/gate scaffold（不写行为测试）**：只落`ToolRegistry::restricted_to`、scope-aware schema、返回独立`ScopeViolation`的`gate_scoped`可编译接口；不得给既有`PermissionDenial`增variant，legacy `register(Box<dyn Tool>)`、`gate`与行为暂保持，运行`cargo test --lib --no-run --locked`。

## 2. Execution scope纯逻辑（强制TDD；接口停点）

- [x] 2.1 **RED（只写测试）**：测试每次root UUID唯一、child直接parent关系、scope clone保identity、legacy两轮root不同；当前scaffold应以断言失败落红，非编译错。
- [x] 2.2 **RED（只写测试）**：用`CancellationToken`受控测试parent cancel传播到child/grandchild、child cancel不影响parent/sibling、cancel-before-wait立即完成、已取消parent派生child立即取消；不得用sleep判时序。
- [x] 2.3 **RED（只写测试）**：覆盖iteration/deadline/depth派生，允许保持/收紧，拒绝增大iteration、推迟/移除parent deadline、depth=0继续派生；用`start_paused`/`advance`锁deadline，不等待真实时间。
- [x] 2.4 **RED（只写测试）**：覆盖tool-name与permission-level子集成功，未知/重复/parent未允许的tool或level整体失败，错误不返回部分scope。
- [x] 2.5 **用户确认停点**：贴出§2.1–2.4测试代码与原始RED输出；等待用户明确批准后才能进入GREEN。
- [x] 2.6 **GREEN**：实现root/clone/derive identity、`CancellationToken::child_token`、budget验证、capability子集验证与可区分错误，使§2.1–2.4全绿；不加入token usage预算或scheduler字段。
- [x] 2.7 **regression**：多次运行scope targeted tests，确认无sleep/flaky；运行`cargo test --lib --no-run --locked`证明新类型未破坏现有Agent/Tool/Permission编译。

## 3. 共享/受限ToolRegistry与scope permission clamp（强制TDD；新权限路径停点）

- [x] 3.1 **RED（只写测试）**：给带内部原子计数的mock Tool建立parent/受限registry，锁同一实例共享；请求逆序名称时schema仍按parent顺序；空集合成功；未知/重复名称整体失败；legacy Box注册、重名与schemas零回归。
- [x] 3.2 **RED（只写测试）**：锁mode filter与scope capability取交集；模型/调用方硬发被隐藏的已注册工具时，ReadOnly/Yolo/allowlist/异常Allow均不能绕过，`gate_scoped`返回独立ScopeViolation且decider/preview/execute调用数均为0；legacy gate类型/variant/矩阵不变。
- [x] 3.3 **用户确认停点**：贴出§3.1–3.2测试代码与原始RED输出；这是新权限路径首次成型，等待用户明确批准后才能进入GREEN。
- [x] 3.4 **GREEN**：registry内部迁为`Vec<Arc<dyn Tool>>`并保持`register(Box<dyn Tool>)`边界；实现先完整验证、再按parent顺序clone Arc的`restricted_to`，使§3.1全绿。
- [x] 3.5 **GREEN**：让同一`ExecutionCapabilities::allows(tool)`驱动scope schema、dispatch前clamp与`gate_scoped`；新增安全ScopeViolation content，scope检查先于ReadOnly、Network preview及decider，使§3.2全绿。
- [x] 3.6 **regression**：重跑ToolRegistry注册/重名/schema/schemas_for、12工具分类、四权限级、Network preview、四种PermissionMode、allowlist/always-allow与Plan纵深全组，确认root全能力路径逐字段零回归。

## 4. Agent scoped正常路径、identity observer与termination边界（强制TDD；Loop停点）

- [x] 4.1 **RED（只写测试）**：用相同Mock脚本比较legacy与等价root scoped入口的Provider请求、history、tool outcomes和返回值；锁有效iteration=`min(agent,scope)`、无deadline legacy不新增超时、每个scoped observer事件携同一identity、并发两个run可按run_id归属；另编译型fixture证明旧observer仅实现legacy callbacks仍可用且事件不重复。
- [x] 4.2 **RED（只写测试）**：Provider/context preparation等待期间cancel/deadline应drop future、返回可区分ScopedAgentError、不写半条Assistant并回滚尚未提交的当前User turn；forced-final等待期间也可取消且不得误报MaxIterations/Provider timeout，既有AgentError variant集合不变。
- [x] 4.3 **RED（只写测试）**：受控串行工具/permission future entered后cancel，锁当前及后续occurrence按模型顺序各一synthetic canceled/deadline ToolResult、后项不execute、不发下一轮Provider、termination后无finished/usage/Idle。
- [x] 4.4 **RED（只写测试）**：受控ParallelSafe批次先发布前缀，再cancel其余；锁前缀原样、未发布项全转synthetic error、重复call id按occurrence不去重、ready但未发布结果被丢弃、sibling迟到结果不入history/observer。
- [x] 4.5 **用户确认停点**：贴出§4.1–4.4测试代码与原始RED输出；等待用户明确批准后才能改Agent Loop。
- [x] 4.6 **GREEN**：将唯一Loop实现迁入scoped入口，legacy入口仅创建新root并委托；接通context、主Provider与forced-final的biased termination select、effective iteration与identity observer，使§4.1–4.2全绿。
- [x] 4.7 **GREEN**：让dispatch返回termination结果；串行permission/execute和并行batch等待统一接scope，按“已发布前缀”补synthetic ToolResult且termination后停止observer，使§4.3–4.4全绿。
- [x] 4.8 **blocking characterization**：用可释放OS watchdog证明已进入`spawn_blocking`的读取在scope取消后可自然完成但迟到outcome不公开，进程级Semaphore上限仍≤4；测试失败路径必须release closure并abort driver。
- [x] 4.9 **regression/refactor**：在全绿前提下清理重复select/收口代码；重跑max_iterations、Provider errors、ContextStrategy、thinking、usage、mode snapshot、unknown tool、serial/parallel排序/屏障/error isolation与observer既有全组。

## 5. TUI root cancellation接线（无视觉变化；事后回归）

- [x] 5.1 为每个Prompt创建独立root scope和cancel handle；把`run_agent_task`的Interrupt路径改为cancel pinned scoped run、await Agent内核收口、保存working history并只发唯一`Interrupted`，不再用TUI suffix helper补当前turn。
- [x] 5.2 保留`complete_interrupted_tool_results`供loaded-session normalization使用；用真实旧session fixture锁`--continue`与picker `--resume`仍补升级前dangling occurrence，raw `SessionStore::load`、磁盘与session JSONL不变。
- [x] 5.3 扩展TUI集成测试：Provider等待、permission等待、双并行工具entered后Interrupt均无trailing finished/Idle；Provider提交Assistant前中断会从下一轮模型history隔离旧Prompt，已提交工具轮的working history occurrence完整；新Prompt/排队Prompt可继续；run正常完成与Interrupt同tick只能产生一个terminal event。
- [x] 5.4 对照`设计规范/02-布局与交互.md`既有Interrupt/queue状态机与`设计规范/03-组件清单.md`C5/C10，运行现有Midnight/Daylight Interrupted工具卡`TestBackend + insta`快照；确认文案、布局、theme token逐字节零churn且无`.snap.new`，本change不新增视觉快照。

## 6. 文档、依赖与范围锁定

- [x] 6.1 更新`技术方案/mysteries-agent技术方案.md`路线图：1.5 helper已被通用execution scope取代；明确cancellation方向、预算/capability和它仍不等于subagent实现。
- [x] 6.2 更新README架构说明与CHANGELOG `[Unreleased]`：如实描述“Agent run具备内核级可取消/不可扩权scope基础”，不得宣称已支持subagent、后台任务或硬取消blocking IO。
- [x] 6.3 审查`Cargo.toml`/`Cargo.lock`/`cargo tree -i tokio-util`：只新增已解析`tokio-util` direct edge/`rt` feature，无无关crate升级；运行`cargo-audit audit --deny unsound --file Cargo.lock`并按现有spec区分0 vulnerability/0 unsound与允许warning。
- [x] 6.4 核对config、CLI grammar、Provider/Tool wire、Network preview、PermissionMode矩阵、session JSONL、ToolOutcome与TUI snapshot metadata均未改变；`delegate_task`、subagent、MCP、child session、token总预算等标识不得出现在生产注册表或用户命令中。

## 7. 自动化门禁与审查

- [x] 7.1 **targeted**：scope identity/cancellation/budget/capability、registry restricted/share、scope schema/gate、legacy/scoped Loop、Provider/context/permission/serial/parallel/forced-final termination、blocking迟到结果、TUI Interrupt/queue/session normalization全部稳定全绿。
- [x] 7.2 **format + full Rust**：运行`cargo fmt --all -- --check`；随后在每个独立PowerShell shell设置`$env:CARGO_TARGET_DIR='target/codex-agent-scope'`，依次运行`cargo clippy --all-targets --locked -- -D warnings`、`cargo test --locked`、`cargo build --release --locked`。隔离target仍被锁时报告并停止，不得kill进程或改用默认target。
- [x] 7.3 **OpenSpec**：运行`openspec validate add-agent-execution-scope --strict`、`openspec validate --all --strict`，确认status为4/4、apply instructions remaining与未勾checkbox一致。
- [x] 7.4 **范围/快照**：运行`git diff --name-only`、`git diff --check`并审查未跟踪文件、dependency diff、snapshot diff；不得修改`.ai_history`（仅archive且用户审阅后写），不得遗留`.snap.new`或无关fmt churn。
- [x] 7.5 **对抗式审查**：重点检查scope扩权、cancel/complete竞态、duplicate id occurrence、未发布ready result、permission/preview旁路、deadline与Provider timeout混淆、legacy wrapper scope复用及Arc共享状态；修复后重跑受影响targeted与完整门禁。

## 8. 真机核验（用户专属；实施Agent不得代勾）

- [x] 8.1 TUI正常轮：在Normal/AcceptEdits/Yolo/Plan分别完成一轮既有任务，确认权限、Network preview、工具卡、并行读取与最终回答和v1.2.0一致。
- [x] 8.2 Provider等待中断：提交会产生较长模型输出/思考的任务，在工具开始前按Esc；只出现一次“已中断本轮”，随后立即提交新任务可正常完成，无迟到文本/status。
- [x] 8.3 串行/权限回归：在权限框按Esc确认仍只拒绝当前调用；另批准一个**无副作用**的延时`run_shell`，待进入Executing且modal已关闭后按Esc中断，只验证Agent history/卡片无迟到结果且下一轮正常，不宣称OS进程被终止。不要把modal内Esc误当作turn cancellation测试。
- [x] 8.4 并行读取中断：让至少两个大目录`grep/glob`同时运行后按Esc；所有卡片收口、无Provider dangling tool result错误，立即发新Prompt可继续；后台读取即使自然结束也不得产生迟到Done。
- [x] 8.5 Session恢复：中断后退出并分别用`--continue`与picker `--resume`恢复，确认无Running残留、无重复interrupted结果、恢复后首轮Provider正常。

## 9. Archive checklist（不计入apply进度）

仅在全部checkbox完成、用户完成真机核验并明确发起archive后：

1. 确认artifacts 4/4、apply remaining=0，四份delta与实现一致。
2. 展示`agent-execution-scope`新增spec及`agent-loop`、`tool-system`、`permission-gate`三份delta sync摘要，取得用户选择后同步主spec并更新Purpose。
3. 执行archive move；按实际数量更新README，不提前写预测值。
4. 按AGENTS.md起草本change决策记录，交用户审阅，并与archive进入同一提交。
5. 运行`openspec validate --all --strict`、`git diff --check`和archive路径/数量复核。
