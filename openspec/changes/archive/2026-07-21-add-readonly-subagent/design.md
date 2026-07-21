## Context

上一 change 已提供 `AgentExecutionScope::derive_child`、parent→child cancellation、iteration/deadline/depth预算、capability单调收窄、受限 `ToolRegistry`和带 `RunIdentity` 的observer callback。当前产品仍有三个接线缺口：

1. `Tool::execute(args, ToolContext)`拿不到当前scope或observer，普通工具无法安全派生child。
2. `Agent`直接持有Provider/model；若`delegate_task`在assembly时捕获它们，TUI运行时切换后child会使用旧配置。
3. 四个本地读取工具允许绝对路径和`..`；对自主child仅做名称/权限限制仍不能阻止workspace外读取。

现有基础可直接复用：Provider已是`Arc<dyn Provider>`，`ToolRegistry::restricted_to`共享同一`Arc<dyn Tool>`，Agent Loop已有`ParallelSafe`上限4、occurrence顺序发布与统一termination收口。TUI已有通用C5工具卡和root-only会话history；本change不需要新布局或新session模型。

## Goals / Non-Goals

**Goals:**

- 交付一个可在TUI和headless中使用的`delegate_task`，把只读调研交给临时child Agent。
- child只使用四个workspace内读取工具，不能递归、扩权、触网、修改、执行、提问或改变Plan状态。
- child继承调用当下的Provider/model和parent cancellation，具有固定iteration/deadline预算。
- 连续委派复用既有有界并行和occurrence顺序，不新增调度器。
- child内部history、stream和工具事件不污染parent transcript/session；token usage仍计入当前turn。
- 保持legacy Agent入口、root Agent绝对路径语义、配置、CLI grammar、Provider wire、permission矩阵和现有快照兼容。

**Non-Goals:**

- 不提供递归Agent graph、可写/可执行/可联网child、后台任务、child session、恢复/重连或独立subagent UI。
- 不实现token总预算、按用户配置的child模型/并发/timeout，也不增加新dependency。
- 不把prompt当作安全边界；结构化registry、scope、workspace containment才负责fail-closed。
- 不改变普通root Agent当前允许读取workspace外绝对路径的行为；若要全局收紧，应另开兼容性与迁移change。
- 不承诺抵御能在canonicalize与open之间主动替换本地目录项的并发攻击者；handle-relative原子文件访问不在本MVP。
- 不解决既有root Agent读取不可信仓库内容时的一般Prompt injection问题，也不实现MCP或第三方Agent SDK。

## Decisions

### D1 · `delegate_task`是普通`ReadOnly + ParallelSafe`内置工具

新增`src/tool/delegate.rs`（最终文件名可按现有模块风格微调），schema固定为：

```json
{
  "type": "object",
  "properties": {
    "task": { "type": "string", "minLength": 1 }
  },
  "required": ["task"],
  "additionalProperties": false
}
```

空白task在执行边界以`task.trim().is_empty()`判定并返回`is_error`且零Provider调用；非空task只做该判定，不裁剪或改写，child收到原字符串。工具为`PermissionLevel::ReadOnly`，在具有至少一层child depth的Normal/AcceptEdits/Yolo/Plan scope中可见；为`ToolConcurrency::ParallelSafe`，因此它与相邻的其他`ParallelSafe + ReadOnly + !plan_only`工具进入同一个既有work-conserving安全段，不按工具名另行分批。最多4个outer tool future同时in-flight，第5项及后续occurrence等待空位后继续，结果仍按parent模型的tool-call occurrence顺序发布，重复call id不去重。

选择既有batch而不是新建child scheduler/`Semaphore`：所需并发、屏障、取消和顺序契约已经存在，第二套调度器只会制造竞态。固定上限4只约束同时active的outer工作，不限制单轮delegate occurrence总数；单个child最多产生`min(parent.max_iterations, 8)`次tool-enabled Provider调用，触顶时还会有一次既有forced-final调用。成本突发风险在文档中如实说明，不把并发上限误写成调用总量上限。

### D2 · 新增source-compatible `ToolExecutionContext`与scoped执行入口

保留现有必需方法：

```rust
async fn execute(&self, args: Value, ctx: &ToolContext) -> ToolOutcome;
```

为`Tool`增加带default转发的方法（命名可按实现收敛）及默认返回0的`required_child_depth()`元数据：

```rust
async fn execute_scoped(
    &self,
    args: Value,
    ctx: &ToolExecutionContext<'_>,
) -> ToolOutcome {
    self.execute(args, ctx.tool).await
}
```

`ToolExecutionContext`只借用：

- 既有`ToolContext`
- 当前`AgentExecutionScope`
- 当前`AgentObserver`
- 可选canonical read root

Agent串行与并行dispatch统一只调用`execute_scoped`。default实现保证现有第三方/测试Tool无需修改且行为不变；`delegate_task`读取scope/observer，四个fs Tool只在read root存在时增加containment检查。不得把当前scope临时写进共享Tool字段，否则两个并发run会串scope。

`delegate_task.required_child_depth()==1`。schema生成与dispatch MUST 同时验证当前scope remaining child depth；不足时schema omit，模型硬发则在observer/permission/execute前fail-closed。该元数据default为0，因此既有工具不受影响。

`DelegateTaskTool`仍须实现trait要求的legacy `execute`，但该入口缺少scope与observer，MUST 稳定返回`delegate_task failed: scoped execution context required`的ordinary error，且不得panic、自造root scope、创建child或调用Provider。真实委派只允许从`execute_scoped`进入；错误文本仍按D8的统一bounded envelope规则处理。

备选“给`ToolContext`新增字段”被弃：它是公开struct，大量下游struct literal会在minor版本中编译失败。备选“Agent按名称特判delegate_task”被弃：把工具语义泄漏进Loop且后续无法复用。

### D3 · parent与delegate共享同步的Provider/model runtime

引入crate-private `AgentRuntime`（或等价命名），在一个`Arc<RwLock<RuntimeSnapshot>>`中保存`Arc<dyn Provider>`和model。`Agent::new`保持原签名，内部建立runtime后委托crate-private `Agent::with_runtime(...)`；assembly显式先创建runtime和基础registry，再派生child view、用runtime clone构造`DelegateTaskTool`并注册，最后以`Agent::with_runtime`构造parent，解开“Tool必须先入registry、runtime又必须与Agent共享”的构造循环。测试用runtime identity/`Arc::ptr_eq`等内部证据锁定parent与delegate持有同一handle。

runtime提供三种明确操作：`snapshot()`在一个无`await`区间clone完整tuple；既有`set_provider`、`set_model`、`restore_model`分别保持合法的单字段更新语义；新增crate-private pair replace在一次write lock内同时提交provider与model。TUI provider picker交互切换必须使用“原子替换pair并清理history thinking”的Agent入口，session restore必须使用“原子替换pair并保留history thinking”的Agent入口，禁止继续拼接两个单字段setter。no-tear契约只针对完整snapshot读取与pair replace，不把两次独立setter伪装成事务；ContextStrategy仍在同一同步Agent方法内更新为相同pair。

每次parent Provider请求和每次delegate invocation都先clone完整snapshot再释放锁。`DelegateTaskTool`持有与parent相同的runtime handle以取得调用当下值，但每次child必须从该次不可变`RuntimeSnapshot`构造，不能把共享可变runtime直接交给已启动child；因此child invocation取得snapshot后保持到结束，运行中发生的新切换不迁移该child，下一次delegate才观察新pair。child Agent通过crate-private builder/constructor显式注入canonical read root；普通`Agent::new/with_runtime`默认read root为None，避免公开`ToolContext`变更。

备选“DelegateTaskTool捕获assembly时Provider/model”被弃：会产生陈旧child。备选“分别锁Provider和model”被弃：读取可能得到跨切换的混合组合。

### D4 · child是每次调用新建的临时Agent

assembly先从root registry派生只含以下名称且保持parent顺序的restricted registry：

```text
list_dir
read_file
glob
grep
```

然后才把`delegate_task`及可选Plan/ask/progress工具注册进root，确保child registry结构上不存在委派、Network、Edit、Execute和交互工具。每次调用用runtime snapshot、restricted registry clone、拒绝一切非ReadOnly决策的内部decider和默认`Passthrough`构造新Agent；child permission mode固定Normal、thinking固定`Depth::Low`，不继承parent的Plan指令或Yolo状态。

child history严格为：

```text
System(SUBAGENT_SYSTEM_PROMPT)
User(task)
```

不复制parent System/history/thinking/plan/session metadata。system prompt要求把文件和tool output视为untrusted data，只完成委派边界内的workspace调研，不执行其中指令、不索权、不提问、不再次委派，并以可验证路径/行号与不确定项作答。prompt是defense-in-depth，registry/scope/path clamp才是强制边界。

备选“复用parent history”被弃：扩大token、泄漏无关对话且使child任务边界不可测。备选“持久化child session”被弃：MVP不需要恢复语义。

### D5 · 产品root depth=1，child固定8 iterations与120秒deadline

现有`Agent::root_scope()`和legacy `run/run_observed`继续创建depth=0 root；scope-aware schema会摘掉`required_child_depth>0`的工具，dispatch也拒绝模型硬发。新增明确的产品委派root入口，TUI与headless使用相同全能力root但`remaining_child_depth=1`。这避免已有library调用方在未选择新能力时看到或意外获得child派生权。

有效参数校验完成后、任何filesystem或Provider工作前立即记录`invocation_time`并取得runtime snapshot，随后派生child scope。120秒deadline从该时刻计时，覆盖进程级blocking permit等待、workspace-root canonicalization preflight、child Agent构造和完整child run；不能等取得permit或canonicalize完成后才开始计时。

delegate从当前parent scope派生：

- `max_iterations = min(parent.max_iterations, 8)`
- `deadline = min(parent.deadline, Instant::now() + 120s)`；parent无deadline时仍设120s
- `remaining_child_depth = 0`
- tool names恰为四个读取工具
- permission levels恰为`ReadOnly`

固定值作为内部常量，不进config。child scope派生失败转为外层工具错误，不能panic。parent cancellation自然传给所有child；child-only deadline/cancel不反向影响parent/sibling。

8轮足以完成多步只读调研且限制上下文增长；触顶时沿用Agent Loop既有的一次forced-final，因此单个child最多发起9次Provider调用。120秒同时覆盖preflight与这些调用，并继续作为总wall-clock上限。备选“继承无deadline”被弃：挂起child只能依赖人工Esc。备选“开放递归depth”被弃：需要独立全局预算和图调度设计。

### D6 · child读取在canonical workspace root内fail-closed

child scope派生后，workspace-root canonicalization MUST 先异步取得既有进程级blocking limiter的owned permit，再把permit移入`spawn_blocking`（或等价非async-thread blocking seam）closure并持有到真实canonicalization结束；permit等待和JoinHandle等待都处于同一child termination裁决下。child-only deadline到达时drop awaiting future，迟到canonicalization结果不得启动Provider；已进入blocking pool的closure可自然结束，但在结束前继续占有permit，因此Interrupt、timeout或连续turn不得堆出超过4个真实blocking preflight/fs closure。若在permit等待阶段终止则不得spawn closure。parent cancellation/deadline的最终对外结果仍由outer scope裁决；nested select竞态中delegate future即使先构造了临时ordinary outcome，也必须被D8的post-ready checkpoint丢弃，绝不能发布成delegate错误。可新增crate-private泛型blocking helper复用现有limiter，但不得增加dependency或第二个独立pool。四个fs Tool把target canonicalize、containment和实际读取/遍历放在同一个既有`run_blocking_tool` worker/permit内：先解析并canonicalize目标、要求目标等于canonical root或为其descendant，再把该已验证canonical target直接交给底层操作，不重新解析原始输入。目录walker对parent、`.ignore`与`.gitignore`的metadata/content读取同样属于受控I/O：scoped parent discovery MUST 在canonical read root停止；每个可能被加载的规则文件 MUST 在打开、解析前canonicalize并验证仍位于read root，越界symlink必须返回is_error而不是静默忽略。绝对workspace内路径允许；绝对workspace外、`..`逃逸、symlink/junction或ignore规则文件指向workspace外均返回`ToolOutcome{is_error:true}`，静态fixture中的外部内容不得进入child history或Provider请求。

containment使用path component/canonical path语义，不做字符串前缀判断。允许target等于root只表示通过边界检查，不改变底层输入类型：`list_dir/glob/grep`可对root目录成功，`read_file(root目录)`仍按既有目录读取错误结束；`read_file`成功case使用root下文件。scoped walker在同一blocking closure内预验证并构造canonical read root内的有界规则上下文，再从调用者显式target开始actual walk：因此read root内从root到target的普通parent规则与target内嵌套规则保持既有precedence/whitelist语义，但即使parent规则命中target或其ancestor，也不得在到达显式target前将其剪枝；read root外ancestor规则不打开、不解析也不影响结果。root Agent的`read_root=None`，default `execute`与既有绝对路径、gitignore行为逐字段不变。glob/grep不跟随越界symlink；若底层walker行为不能结构性保证，则在产出/读取前再次验证。

备选“只在system prompt写不要越界”被弃：Prompt injection可直接绕过。备选“全局修改resolve_path”被弃：超出本change并构成兼容性变化。

### D7 · child事件隔离，usage按child identity计入parent observer

child使用Noop/Buffered `DeltaSink`，text与thinking不进入parent transcript。它把当前observer直接传给`run_observed_scoped`，因此所有child事件仍带child `RunIdentity`：

- 支持scoped identity的通用observer可观察child status/tool/usage；
- `ChannelObserver`override scoped callbacks，按`parent_run_id`过滤child status/tool事件，但继续把child usage汇入当前turn统计；
- legacy observer的default转发行为保持source-compatible；只有实际使用新`delegate_task`时才可能收到新增的child事件。

Agent Loop主循环与forced-final每次Provider请求前均必须用当前run identity发送`CallingModel`，每次Provider响应的`usage`均必须上送；成功forced-final也按主`agent-loop`既有契约发送`Idle`，Provider error、termination或空final失败路径不发`Idle`。当前实现对root与未来child都遗漏forced-final `CallingModel`/usage/Idle，本change会把它们恢复到主spec要求；除这一组既有spec修复外，不使用`delegate_task`时事件逐项不变。

因此TUI只显示一张outer `delegate_task` C5卡及最终结果，不会发生child call-id碰撞、activity覆盖或流式文本混入。现有legacy observer method签名与default转发保持source-compatible。

若实现发现向通用observer转发child事件会破坏既有公开契约，必须先修订spec，不得用全局Noop静默丢失usage。

### D8 · ToolOutcome把child报告标记为untrusted并严格截断

成功内容固定为：

```text
subagent report (untrusted):
<child final text>
```

先形成完整raw envelope，再按`ToolContext.max_output_bytes`在UTF-8边界整体截断，正确设置`truncated`，`exit=None`。cap足以容纳固定前缀时，成功结果保证包含完整untrusted前缀；cap小于前缀长度时，只允许返回raw envelope的UTF-8安全前缀片段（可为空）、`truncated=true`且不得越过前缀泄漏child文本。空最终文本视为错误。

由`DelegateTaskTool`在parent仍active时产生的参数、unscoped入口、workspace root、scope派生、Provider/Agent、空final和child-only deadline错误，先形成`delegate_task failed:`raw envelope，再按相同bounded规则整体截断并置`is_error=true`。required-depth/unknown/scope dispatch拒绝发生在Tool execute前，沿用既有scope错误；parent cancellation/deadline只由外层Agent Loop生成唯一synthetic interrupted/deadline ToolResult，二者均不得使用delegate error前缀。

串行路径在tool future ready后、发布history/finished observer前，必须再次执行parent scope checkpoint。ParallelSafe路径可先把乱序完成项放入仅内部可见的ready buffer，但每个item成为连续可发布前缀、即将写history/finished observer时 MUST 再次checkpoint；仅这次紧邻同步发布且中间无`await`的检查才是该occurrence的publication linearization point。若它观察到termination，则丢弃当前及所有尚未发布的ready outcome并生成synthetic收口；checkpoint通过后才允许同步发布，之后到达的termination视为晚于该occurrence。入ready buffer不是发布，也不能用较早的checkpoint替代发布前复查。不能只依赖nested biased `select!`，否则child可能在outer termination branch首次poll为Pending后先观察同一token并把取消误映射成ordinary error。

结果不序列化child history、run id或内部工具详情。untrusted envelope降低parent误把报告内文本当新指令的概率，但不宣称消除模型层Prompt injection。

### D9 · TUI仅adapt现有C5卡，session只保存outer occurrence

`delegate_task`使用现有C5 ToolCard的名称、args、running/done/error、output和truncation布局；按`设计规范/03-组件清单.md` C5与既有theme token新增Midnight/Daylight事后快照，现有快照必须零churn。没有child面板、层级树或新快捷键；相对HTML原型归类为adapt。

session只记录parent Assistant中的outer ToolCall及对应ToolResult/transcript卡。child history、scope、identity、usage细项与内部卡不写JSONL。Interrupt时outer running卡沿现有generic收口为Error；`--continue`/`--resume`不恢复或重跑child。

### D10 · TDD按scoped seam、安全边界、delegate Loop和TUI接线分层

本change是headless内核、新工具和新execution路径：

1. 只落可编译的类型、trait default与constructor/test seam scaffold，先锁legacy Tool source compatibility；`DelegateTaskTool`可有不注册、零child副作用且明确未实现的compile-only trait placeholder，blocking helper/canonicalizer/limiter只落可注入签名。Agent串/并行dispatch仍调用旧`execute`，不得提前接通真实scoped context或让任何后续行为oracle提前GREEN。
2. scoped override在真实dispatch中收不到scope/observer/read root、runtime pair replace、child root/depth与workspace containment分别先RED；首次新接口/安全路径RED展示后停点，随后GREEN才把dispatch切到`execute_scoped`。
3. `delegate_task`参数、history/schema/capability、预算、结果与failure RED→GREEN。
4. 用oneshot/Barrier与paused time锁4并发、occurrence顺序、Provider切换、parent cancellation、child deadline和无迟到事件；不用sleep。
5. TUI只做接线后的TestBackend/insta与session集成回归，不先写视觉测试。

不得在同一步同时提交RED测试和实现，不得用硬编码Mock结果绕过真实child Agent Loop。

## Risks / Trade-offs

- **[一次回复可含任意数量delegate occurrence并放大Provider/token成本]** → 4只限制同时active的outer child，第五个及后续仍会执行；每个child最多8个tool-enabled调用加1次forced-final，总调用上界约为`delegate occurrence数 × 9`。不允许递归、不新增可配置放大器，README/CHANGELOG必须如实区分并发与总量。
- **[本MVP没有per-response delegate总数、token总预算或child-only扫描字节上限]** → 这是显式接受的residual resource risk，不宣称总成本/总输出/总内存有固定上界；四fs工具仍沿用既有“先读取/遍历/收集”的语义，仅`read_file`与`grep`按`max_output_bytes`后置截断，`list_dir`与`glob`的既有输出没有该硬上限。当前change只提供active outer≤4、全局blocking≤4、每child≤8+1轮与120秒wall-clock边界。任意新增occurrence cap、`list_dir`/`glob`输出cap或扫描早停值都会改变已批准的第5项等待后继续及工具输出语义，必须在取得产品阈值后另开budget change，不能由实施者临时猜值。
- **[共享runtime锁进入async路径会死锁或拖慢流式请求]** → 只在同步区clone `(Arc<dyn Provider>, String)` snapshot，任何`await`前释放锁；切换与并发delegate测试覆盖撕裂。
- **[workspace root canonicalization在child计时前卡住或取消后遗留closure堆积]** → 参数校验后立即capture invocation/snapshot并派生scope；preflight的进程级blocking permit等待与JoinHandle等待都受child termination约束，permit移入closure并持有到真实结束。stalled canonicalizer测试锁120秒deadline、零Provider、迟到结果丢弃与跨波次真实blocking max-active≤4。
- **[Windows path containment对symlink/junction与大小写处理复杂]** → 两端canonicalize并按Path component比较，使用临时目录真实symlink/junction能力测试；平台不支持创建junction的测试须明确skip原因，不能用字符串替代安全断言。
- **[walker可能通过parent或linked ignore metadata侧读workspace外内容]** → scoped walker关闭越过read root的自动parent discovery；同一blocking closure在actual walk前按既有ignore/hidden pruning逐层枚举walker可能加载的`.ignore` / `.gitignore`（被剪枝目录自身control file仍验证，但不探测其不可达descendant），逐个canonicalize并做containment，外部ancestor规则不生效，内部reachable linked规则越界则fail-closed；root `read_root=None`继续使用既有walker。
- **[canonicalize后到实际open之间仍存在主动并发换链race]** → 检查与I/O位于同一blocking worker并使用已验证canonical target，缩小但不宣称消除namespace race；MVP威胁模型明确不承诺抵御同时修改目录项的本地攻击者，若要handle-relative强隔离需独立change评估平台API/dependency。
- **[canonicalize要求路径存在]** → 四个child工具本就读取既有目标；不存在路径继续返回稳定is_error，不扩大工具语义。
- **[child observer事件污染TUI或usage漏算]** → ChannelObserver按run identity过滤status/tool、保留usage；Mock observer锁事件归属、次数与termination后无迟到。
- **[ParallelSafe delegate完成乱序]** → parent Agent既有ready buffer按occurrence发布；重复id测试保证不按id去重。
- **[parent取消与child deadline同tick或同一次poll竞争]** → outer biased select之外，串行结果及ParallelSafe连续可发布前缀都在紧邻history/observer发布且中间无`await`的位置再次checkpoint parent scope；私有ready buffer不构成linearization。测试只接受对应唯一终态，不同时发布普通child error与synthetic cancellation。
- **[untrusted报告仍可能影响parent模型]** → child prompt、workspace clamp和固定envelope做defense-in-depth；不把模型Prompt injection宣称为已消除。
- **[固定8轮/120秒不适合所有任务]** → MVP以确定性和成本上限优先；真实验证后另开change讨论配置化，不在本次预建。

## Migration Plan

1. 记录assembled Agent、provider/model切换、四fs工具、TUI Interrupt/session与现有快照baseline。
2. 增加只含类型/default/签名的source-compatible scoped Tool/runtime scaffold，证明现有Tool实现和legacy Agent入口可编译；Agent dispatch仍走旧`execute`。
3. 先取得scoped dispatch、runtime pair replace/frozen snapshot与child-only workspace containment的行为RED，批准后再完成对应GREEN与零回归验证。
4. 实现`delegate_task`、受限registry、固定scope预算与结果收口，再接入assembly。
5. TUI/headless改用depth=1产品root；legacy wrappers保持depth=0。
6. 完成并发/cancellation/deadline/observer/session/TUI集成、文档和全量门禁。

回滚时先从assembly移除`delegate_task`并让产品入口恢复depth=0，再回退scoped Tool/runtime扩展；config、session和Provider wire无数据迁移。

## Open Questions

- 无阻塞问题。更深递归、全局subagent并发池、per-response occurrence/token/child扫描预算、child专用model、可写child和独立UI均保留给后续change。
