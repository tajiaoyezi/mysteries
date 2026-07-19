## Context

`Agent::run_observed` 当前直接拥有完整循环：context preparation、Provider、schema、permission gate、串行/并行工具与 forced-final。调用方只能 drop 整个 future；TUI 因此在外层 `tokio::select!` 中抢占 run，再用 `complete_interrupted_tool_results` 猜测并补齐本轮 history。该 helper 已能按 occurrence/FIFO 收口 TUI turn，却无法被 headless 或未来 child Agent 复用，也不能表达 parent→child cancellation、deadline、运行身份或 capability 上限。

现有并行读取进一步暴露这个缺口：`buffer_unordered` future 被 drop 后已进入 `spawn_blocking` 的读取会自然完成，结果虽不再公开，但 Agent 本身不知道应为哪些未发布 occurrence 补结果。技术方案已明确 `ToolConcurrency` 只是 subagent 的调度先例，不是通用 cancellation。

可复用基础已经存在：Provider 是 `Arc<dyn Provider>`；Tool 是 `Send + Sync`；权限门集中；`uuid`、Tokio 与 `futures-util` 已在依赖图。缺口是 ToolRegistry 仍持 `Box<dyn Tool>`、observer 事件没有 run identity、Agent Loop 没有 termination contract。v1.3 的第二个 change `add-readonly-subagent` 将只消费本 change 的能力，本 change 不交付 subagent 工具。

## Goals / Non-Goals

**Goals:**

- 让一次 Agent run 具有稳定 identity、可区分的 cancellation/deadline、iteration/depth预算和单调收窄 capability。
- 让 parent cancellation 传播到 descendant，而 child cancellation 不影响 parent/sibling。
- 在 Agent Loop 内对 Provider、context、permission、串行与并行工具、forced-final 做统一终止与 occurrence 完整收口。
- 让 registry 能无复制地派生受限 view，并以 schema-omit + dispatch clamp + permission clamp 三层阻止越权。
- 保持 legacy `run` / `run_observed`、正常 TUI、Provider wire、session JSONL 与现有工具输出零回归。
- 给下一 change 提供足够的 headless seam，但不预建未使用的 subagent scheduler或UI模型。

**Non-Goals:**

- 不新增 `delegate_task`、child Agent factory、subagent prompt、并发 child scheduler或递归 Agent graph。
- 不允许新的 Network/Edit/Execute 行为，也不改变 Normal/AcceptEdits/Yolo/Plan矩阵。
- 不做 token消费总预算；Provider usage 可能缺失且只在响应后可得，本 change只做 iteration、deadline与child-depth运行预算。
- 不强杀已经进入 Tokio blocking pool 的OS同步工作；只保证future、结果、history和observer收口。
- 不回滚已经发生的Tool副作用，也不保证终止`run_shell`已启动的OS进程；本change的cancellation是Agent编排契约，未来允许subagent使用Execute前必须另行设计process lifecycle。
- 不持久化 run identity、scope、cancellation或child关系，不改变 session/config格式。
- 不新增TUI布局、文案、卡片字段或快照；仅替换当前turn的中断接线，旧session normalization保留。
- 不实现MCP、OAuth或发布目标扩展。

## Decisions

### D1 · `AgentExecutionScope` 是一次 run 的不可扩权上下文

新增概念类型（最终命名可按Rust惯例微调，但职责不得合并回TUI）：

```rust
struct RunIdentity {
    run_id: Uuid,
    parent_run_id: Option<Uuid>,
}

struct ExecutionBudget {
    max_iterations: u32,
    deadline: Option<tokio::time::Instant>,
    remaining_child_depth: u32,
}

struct ExecutionCapabilities {
    tool_names: BTreeSet<String>,
    permission_levels: BTreeSet<PermissionLevel>,
}

struct AgentExecutionScope {
    identity: RunIdentity,
    cancellation: CancellationToken,
    budget: ExecutionBudget,
    capabilities: ExecutionCapabilities,
}
```

scope clone只复制同一次run的观察/控制handle并保持identity；child只能经`derive_child`创建，取得新UUID、直接parent id和child token。派生API接收显式预算/capability请求并逐项验证：tool names和permission levels必须是parent子集，iteration不得变大，deadline不得变晚或移除，depth必须有剩余。任一越界整体返回具名错误，不静默`min/intersection`，避免调用方误以为child获得了其请求能力。

Agent自身`max_iterations`仍是配置硬上限；scoped run使用`min(agent.max_iterations, scope.max_iterations)`。legacy wrapper建立“当前registry全部工具+四权限级、无deadline、depth=0”的root scope，既保持旧行为，也确保旧入口不能意外派生child。第二个change会为产品root显式配置depth=1。

备选“只传CancellationToken，其他参数继续散落在Agent/ToolContext”被弃：无法表达identity和不可扩权派生，第二个change仍会重新设计。备选“把scope序列化进session”被弃：run控制是瞬时态，提前改变wire扩大迁移面。

### D2 · 直接使用 `tokio-util::sync::CancellationToken`

新增直接依赖`tokio-util = { version = "0.7", features = ["rt"] }`。当前`Cargo.lock`已由现有网络栈解析`tokio-util 0.7.18`，预计只增加根package direct edge与所需feature，不引入新crate/version；实施仍须用lockfile diff和`cargo tree`确认。

选择它的理由是`child_token()`精确提供所需方向：parent cancel传播到descendant，child cancel不反向；同时处理cancel-before-wait与注册竞态。execution policy、budget、history收口、权限和Agent编排仍全部自实现，未引入Agent SDK/Framework。

备选“`Arc<AtomicBool>+Notify`自写树”被弃：parent链、无丢失唤醒与child-only cancel容易产生竞态。备选“所有scope共享同一token”被弃：child cancel会误杀parent/sibling。若实施现场证明现有锁图无法在不升级其他crate的前提下启用`rt` feature，必须停止并修订design，不得临时改用轮询。

### D3 · 新增 scoped入口，legacy入口只做root adapter

Agent增加`run_scoped`与`run_observed_scoped`（或等价清晰命名），显式接收`&AgentExecutionScope`。既有`run`/`run_observed`不改调用参数，内部各创建全能力root scope后委托scoped实现。所有正常逻辑只保留一份，禁止维护“旧循环+新循环”双实现。

新增独立`ScopedAgentError::{Agent(AgentError),Cancelled,DeadlineExceeded}`，与`ProviderError::Timeout`、`MaxIterations`分离；不得给公开`AgentError`增加variant，避免v1.3 minor破坏下游exhaustive match。scope termination helper返回内部`StopReason::{Cancelled,DeadlineExceeded}`；每个异步边界用biased select让已可见termination优先，防止“同一调度tick里取消有时接收响应、有时丢弃”的测试漂移：

```rust
tokio::select! {
    biased;
    reason = scope.terminated() => Err(reason),
    value = operation => Ok(value),
}
```

context strategy（含自动压缩）、主Provider、permission gate、tool execute与forced-final均经过同一helper。同步纯逻辑之间在发observer、写Assistant、启动下一工具前做快速`is_terminated`checkpoint。legacy wrapper建立不暴露cancel handle、无deadline的root scope；普通错误从`ScopedAgentError::Agent`取回原`AgentError`，两个termination variant在该私有root上结构性不可达，并用测试锁定。

备选“给每个Provider/Tool trait增加token参数”被弃：会把Agent生命周期泄漏进全部实现并造成不必要的公共trait churn；drop async future已能取消可取消IO，blocking边界另有明确限制。

### D4 · Agent以“已发布前缀”作为cancellation事实边界

每个Provider回复写入`Assistant{tool_calls}`后，dispatch维护当前Assistant的occurrence序列与已发布前缀。正常路径继续按模型顺序公开。termination一旦被接受：

1. 不再启动任何工具、permission或Provider操作。
2. 保留termination前已写入history并上报observer的前缀结果。
3. 丢弃所有尚未发布的真实/ready结果；为剩余occurrence按模型顺序写一个synthetic is_error ToolResult。
4. canceled内容固定沿用`tool call interrupted before completion`；deadline使用独立稳定内容`tool call deadline exceeded before completion`。
5. synthetic结果只写history，不发tool-finished；termination后不发usage/Idle/status。
6. 返回对应AgentError，不进入下一轮或forced-final。

并行批次的ready buffer只在正常checkpoint发布连续前缀。即便后项已物理完成但尚未公开，termination优先后也按canceled/deadline处理；这牺牲一个不可见结果，换取history、observer和TUI的单一边界。已启动`spawn_blocking` closure可能继续持进程级permit，自然结束后结果被drop；不允许为了“硬取消”杀线程或解除全局上限。

Provider/context在产生Assistant前被取消时不写半条Assistant，并从模型history回滚本次run入口处最后一个、尚无后继Assistant的User turn。该回滚只隔离下一轮Provider上下文，不删除TUI transcript中的用户输入、partial thinking或Interrupted展示。若Assistant已经写入，则以“写入history”为提交点：保留User与Assistant；无tool_calls时正常文本可返回，有tool_calls时按上述occurrence规则收口。测试必须锁定checkpoint，避免写入后又改报canceled；实现时应在写Assistant前检查termination，写入和无工具终止之间不await。普通Agent/Provider错误不触发该回滚，仍保留User供错误诊断与重试。

备选“继续由每个调用方补history”被弃：headless/child无法统一。备选“把未发布ready outcome写history再取消其他项”被弃：会在termination后产生无对应observer的Done结果，并让TUI卡片状态与history分裂。

### D5 · `ToolRegistry` 内部迁为 `Arc<dyn Tool>`，保留Box注册API

registry内部从`Vec<Box<dyn Tool>>`迁为`Vec<Arc<dyn Tool>>`并实现廉价Clone；`register(Box<dyn Tool>)`在边界转为`Arc::from`，因此现有生产装配和测试调用无需批量改签名。`get`仍返回`&dyn Tool`，schemas与插入顺序不变。

新增`restricted_to(names)`：

- 先验证请求无重复且每项存在；任一失败不返回部分registry。
- 成功后按parent插入顺序clone对应Arc，而非按请求顺序重排。
- 空集合得到空registry。
- stateful Tool共享同一对象；本change不尝试clone trait object。

scope capability仍独立存在：受限registry用于构造child；同一root registry上的scoped run还会用capability过滤schema和dispatch。双层设计防止第二个change装配错误时仅靠schema隐藏。

备选“保持Box并让受限view借用parent”被弃：未来`delegate_task`会把child run作为并发future，借用view增加生命周期耦合且无法安全跨spawn边界。备选“重新调用default_registry构造child”被弃：会复制有状态工具并可能错误注册Network/Edit/交互工具。

### D6 · capability clamp顺序固定为 schema → dispatch → scoped gate

Provider看到的schema为`registry insertion order ∩ mode filter ∩ scope tool names ∩ scope permission levels`。Provider仍可能硬发隐藏工具，因此dispatch在observer started、Plan纵深检查和permission之前再次检查scope。允许项进入`gate_scoped`；gate先检查scope，再执行ReadOnly直放或既有Network/decider路径。

新增独立`ScopeViolation(reason)`，`gate_scoped`返回`Result<PermissionGateOutcome, ScopeViolation>`；不得给公开`PermissionDenial`增加variant。root兼容`gate`保持原签名和variant，scoped路径先做capability检查，再复用既有gate。拒绝顺序固定：

```text
unknown registry tool
  → scope tool/level clamp
  → Plan / plan_only纵深
  → Network preview + PermissionDecider
  → execute
```

这样Yolo、allowlist、AllowAlways和异常decider都没有机会扩权；scope禁止Network时甚至不计算专用preview。reason只含tool name/permission level，不含args或凭据。

备选“只派生受限registry、不改permission gate”被弃：未来装配错误或共享root registry的child可绕过。备选“静默交集child请求”被弃：隐藏配置错误且不利审计。

### D7 · 新增带identity的scoped observer callbacks，legacy方法不改签名

`AgentObserver`保留现有`on_status/on_tool_call_started/on_tool_call_finished/on_usage`签名，并新增对应的`on_scoped_*(&RunIdentity, ...)` default方法；每个default实现忽略identity并转发到legacy方法。Agent scoped实现只调用scoped方法，因此旧observer无需修改仍收到原事件，新observer可override scoped方法取得identity。Agent在一次run内复用同一identity，future child可共享同一observer并按run_id归属。`AgentStatus`本身不嵌identity，避免状态类型与生命周期控制耦合。

这是source-compatible的trait扩展：`ChannelObserver`沿用legacy方法保持v1.2 TUI不变，RecordingObserver override scoped方法记录identity。第二个change可新增subagent-aware observer adapter，无需再次修改Agent trait。

备选“直接修改现有callback签名”被弃：`mysteries`公开lib模块，会破坏下游trait实现。备选“只新增on_run_started”被弃：并发child的后续交错事件仍无法归属。备选“把identity塞进每个AgentStatus但不放tool/usage”被弃：信息不完整。

### D8 · TUI由“抢占drop+外部补齐”迁为“cancel+等待内核收口”

每个Prompt创建root scope与可clone cancel handle。`run_agent_task`仍以select等待run或Interrupt；Interrupt arm调用cancel，然后继续await同一个pinned scoped run，预期得到`ScopedAgentError::Cancelled`。Agent返回后保存已收口working history并只发一次`AgentEvent::Interrupted`。若Provider尚未提交Assistant，working model history已回滚旧User，下一条排队/新Prompt不会继续旧任务；UI transcript仍保留旧输入与Interrupted。若run先正常完成则沿用TurnComplete/Error分流。

`complete_interrupted_tool_results`不再用于当前turn主路径，但保留给`normalize_loaded_session`，处理升级前磁盘中的dangling occurrence。`AppState.apply(Interrupted)`、Running卡Error文案、queue gate、session wire和raw load均不改。ChannelObserver在scope termination后收不到finished/Idle，因此现有“无trailing event”契约继续成立。

为防Interrupt和run completion同tick竞争，测试只锁合法的原子结果：要么正常TurnComplete且完整history，要么唯一Interrupted且完整canceled history；不得出现两种terminal event或dangling occurrence。已经确定工具entered后再发Interrupt的测试则必须稳定走Interrupted。

备选“发送cancel后不await run，继续调用旧helper”被弃：仍无法证明Agent内核收口，也会让future晚到访问working借用。备选“把TUI interrupt channel传进Agent”被弃：内核绑定前端协议。

### D9 · TDD按接口、scope纯逻辑、Loop termination与TUI接线分层

本change属于headless内核且新增接口/权限路径，按AGENTS.md严格RED→GREEN：

1. 先落只为编译的类型/API scaffold，不实现目标行为。
2. scope identity/派生/cancellation/budget/capability纯逻辑RED；这是新接口首次成型，展示原始断言失败后停点。
3. registry共享/受限view与scoped gate RED→GREEN。
4. 用oneshot/Barrier替代sleep构造Provider、permission、serial tool与parallel batch cancellation；锁history occurrence、future drop、无迟到observer和forced-final。
5. TUI只做接线后的集成回归，不改渲染，不生成或接受snapshot。

timeout测试用`tokio::time::pause/advance`或`start_paused`虚拟时钟；并发进入点使用per-call oneshot ack，失败清理必须release或abort driver。blocking closure由OS watchdog保证失败路径释放，不宣称closure被硬取消。

## Risks / Trade-offs

- **[新增direct dependency扩大维护面]** → 仅直接声明lockfile已有的`tokio-util 0.7.18`并只开`rt`；记录理由，运行cargo tree/audit，lockfile不得出现无关升级。
- **[取消后blocking读取仍占permit]** → 仅既有无副作用的ParallelSafe读取会进入该路径；全局Semaphore保持上限，迟到结果与observer被丢弃，并在文档中如实说明非硬取消。
- **[已启动的外部进程可能在Agent取消后继续]** → 不改变`run_shell`既有process lifecycle，真机只用无副作用延时命令验证无迟到结果；v1.3只读subagent禁止Execute，未来开放前另做kill-on-drop/process-group change。
- **[scope与registry双重过滤产生规则漂移]** → 同一`ExecutionCapabilities::allows(tool)`纯函数供schema、dispatch和gate使用；交叉测试硬发隐藏工具。
- **[scoped observer扩展导致重复或漏事件]** → Agent只调用scoped callbacks，default单向转发legacy方法；Recording覆盖identity、Channel沿用legacy，并锁每个逻辑事件恰好一次。
- **[TUI等待cancel收口可能比直接drop稍慢]** → async Provider/tool future立即drop；只做内存synthetic结果后返回，不等待detached blocking closure。
- **[deadline与Provider自身timeout混淆]** → 独立AgentError与稳定测试；scope deadline由外层select裁决，Provider attempt timeout仍返回ProviderError::Timeout。
- **[预算抽象过早]** → 首版只纳入Agent已能精确执行的iterations/deadline/depth，不加入usage缺失时无法可靠执行的token预算或child scheduler。
- **[Arc迁移意外改变工具状态隔离]** → parent/受限registry明确共享是目标；默认registry每次构造仍生成新实例，现有装配隔离不变。

## Migration Plan

1. 记录无改动baseline：相关targeted tests、全量lib、dependency tree与现有TUI Interrupt行为。
2. 增加dependency和可编译scope/error/scoped API/observer参数scaffold；legacy路径仍跑旧逻辑，证明编译与既有测试可恢复。
3. 按TDD完成scope纯逻辑、registry Arc+restricted view、scoped gate与schema clamp。
4. 把唯一Agent Loop迁入scoped实现，依次接context/Provider/permission/serial/parallel/forced-final termination与history收口。
5. 将TUI当前turn接到root scope cancellation，保留loaded-session normalization；运行既有Interrupt/queue/session测试，确认无snapshot churn。
6. 更新技术方案、README与CHANGELOG `[Unreleased]`，执行完整Rust/OpenSpec/security/range gates和用户真机Interrupt验证。

回滚时可先让legacy wrapper继续建立全能力root、关闭TUI scoped cancel接线，再回退Agent Loop与registry内部Arc；session/config无数据迁移。若第二个change尚未开始，本change可整体revert，不影响v1.2.0文件格式。

## Open Questions

- 无阻塞问题。`add-readonly-subagent`中root允许的child depth、subagent固定并发上限、child system prompt与最终ToolOutcome格式保持下一change决定；本change只提供可验证的运行控制与不可扩权seam。
