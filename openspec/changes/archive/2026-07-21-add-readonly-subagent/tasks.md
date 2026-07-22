# Tasks — add-readonly-subagent

执行边界：本change交付单层、只读、临时`delegate_task` child Agent；不实现递归、Network/Edit/Execute child、后台任务、child session、token总预算、独立subagent UI或MCP。headless Agent Loop、Tool trait、新内置工具与child-only路径安全强制RED→GREEN；允许先落只含类型/default/签名的可编译scaffold，但新trait seam的dispatch消费与新工具行为首次RED必须展示原始失败并等待用户批准。TUI只做事后回归。Windows验证统一使用隔离的`CARGO_TARGET_DIR=target/codex-readonly-subagent`，不得kill用户进程或用默认target绕过锁。

## 1. Baseline与可编译scaffold

- [x] 1.1 在无生产实现改动时运行`$env:CARGO_TARGET_DIR='target/codex-readonly-subagent'; cargo test --lib --locked`，记录通过/忽略数；运行现有provider/model切换、四fs工具、Agent scope/parallel、TUI Interrupt/session与C5快照targeted baseline，并记录当前forced-final请求/响应未上送`CallingModel`/usage/Idle这一code与主`agent-loop` spec偏差，禁止把它误记为本change引入的回归。
- [x] 1.2 用`git diff --name-only`与`git ls-files --others --exclude-standard`确认当前工作区只含本change规划artifacts；记录`default_registry`、assembled registry与TUI tool count的当前事实，禁止把旧测试假设当设计。
- [x] 1.3 只落可编译的`ToolExecutionContext`类型、带default转发的`Tool::execute_scoped`声明与默认0的`required_child_depth`元数据；Agent串/并行dispatch仍调用旧`execute`，不得传递真实scoped context、实现scope消费、depth/path clamp或delegate行为，运行`cargo test --lib --no-run --locked`恢复编译。
- [x] 1.4 只落crate-private共享`AgentRuntime`、完整snapshot与pair-replace方法签名、`Agent::with_runtime(...)`assembly构造seam、child read-root builder/constructor、产品depth=1 root入口，以及`delegate_task`模块/常量/构造器、可注入canonicalizer/limiter与crate-private泛型blocking helper签名。为后续测试编译可提供不注册、零child/Provider/fs副作用且明确未实现的`DelegateTaskTool` trait placeholder；它不得返回成功、不得匹配后续metadata/result oracle。`Agent::new`委托内部runtime但公开签名保持，legacy `root_scope/run/run_observed`不变，scaffold不得注册可用delegate或宣称行为完成。

## 2. Scoped Tool seam与runtime snapshot（强制TDD；接口停点）

- [x] 2.1 **RED（只写测试）**：先锁只实现旧`execute`的Tool仍可编译、default required depth=0且直接调用default scoped方法只转发一次；再用override `execute_scoped`的probe Tool要求Agent串行/ParallelSafe dispatch传入真实scope identity、observer与read root，两个并发run共享同一Tool时不得串值。由于§1.3仍走旧`execute`，probe必须以“scoped override未被调用”的正确原因RED。
- [x] 2.2 **RED（只写测试）**：锁`AgentRuntime::snapshot`一次读取同一Provider/model tuple，assembly parent与delegate持有同一runtime Arc；单字段`set_provider`、`set_model`、`restore_model`各只原子更新对应字段；crate-private pair replace在受控并发读取下不得观察旧Provider+新model或反向组合，锁不得跨`await`持有，已clone snapshot在后续replace后保持不变。
- [x] 2.3 **用户确认停点**：贴出§2.1–2.2测试代码与原始RED输出，明确展示scoped override尚未被dispatch调用及pair replace尚未实现；这是新Tool trait seam首次成型，等待用户明确批准后才能进入GREEN。
- [x] 2.4 **GREEN**：把Agent串/并行dispatch统一切到source-compatible scoped入口，实现每future上下文传递、共享runtime snapshot与单次write-lock pair replace，使§2.1–2.2全绿；不得给公开`ToolContext`加字段或在Tool共享字段暂存当前scope。
- [x] 2.5 **regression**：重跑所有既有Tool实现、registry、serial/parallel batch、单字段及pair provider/model hot-swap、ContextStrategy更新与legacy observer测试；除§1.1已记录且留待§6修复的forced-final `CallingModel`/usage/Idle偏差外，证明未使用delegate时逐项零回归。

## 3. Child-only workspace containment（强制TDD；安全边界停点）

- [x] 3.1 **RED（只写测试）**：四fs工具分别覆盖read root内relative/absolute成功、absolute外部与`..`逃逸失败及不存在路径稳定is_error；`list_dir/glob/grep`覆盖canonical root目录自身成功，`read_file`覆盖root下canonical file成功并确认传root目录时通过containment后返回既有directory-read error而非escape error。canonicalize、containment与实际I/O必须处于同一blocking worker/permit，越界失败发生在目标content read/walk前且permit正常释放。
- [x] 3.2 **RED（只写测试）**：用临时workspace与外部目标覆盖file symlink、directory symlink及Windows可用时junction逃逸；静态fixture的外部marker不得进入ToolOutcome，底层目标read/walk计数为0。平台能力不足时只允许带原始OS原因的显式skip，不得把字符串前缀测试冒充链接安全测试。
- [x] 3.3 **RED（只写测试）**：read root为None时，普通root四fs工具读取workspace外absolute路径、gitignore、truncation、error与`ParallelSafe`分类逐字段保持baseline。
- [x] 3.4 **用户确认停点**：贴出§3.1–3.3测试与原始RED输出；说明canonical containment与root兼容失败点，等待批准后才能实现。
- [x] 3.5 **GREEN**：实现child read root canonicalization；四fs在同一blocking worker/permit内使用canonical Path component containment并把已验证target直接交给底层读取/遍历，不做字符串前缀判断或重新解析原始输入；root None直接走既有execute。
- [x] 3.6 **regression**：多次运行静态workspace escape、symlink/junction及四fs全组；确认目标I/O只发生在验证后、blocking permit正常释放且root既有行为全绿；如实保留“不抵御并发namespace替换”的威胁模型。
- [x] 3.7 **RED（安全审查补充）**：覆盖scoped `list_dir/glob/grep`不得读取canonical read root外ancestor `.ignore/.gitignore`，workspace内普通parent/nested规则及hidden descendant target保持既有语义；workspace内`.ignore`与nested `.gitignore`链接到外部时在actual target walk前fail-closed、marker不泄漏且目标I/O计数为0。真实link能力不足时记录原始OS skip；RED必须以外部parent规则仍隐藏workspace probe的正确原因失败。
- [x] 3.8 **GREEN/regression**：scoped walker关闭越过read root的parent discovery，在同一blocking closure中先canonicalize并验证所有可能加载的ignore控制文件，再执行保留read root内parent/nested规则的actual walk；重跑新增安全组及四fs全组，证明root `read_root=None`逐字段零回归。

## 4. delegate_task元数据、child装配与不可扩权（强制TDD；新工具停点）

- [x] 4.1 **RED（只写测试）**：锁`delegate_task`名称、description、严格`{task}`schema、`ReadOnly`、`ParallelSafe`、非plan-only、required child depth 1与不可授权Network preview；null/缺失/类型错/空白/额外字段均保持outer started/finished各一次并返回is_error，同时零child scope/child observer/Provider/child execute；空白仅以`trim().is_empty()`判定，非空task传给child时逐字节保持原值。使用足够cap直接调用legacy `execute`时稳定返回is_error且reason为`scoped execution context required`，不得panic、自造scope或产生任何child副作用；完整error envelope与极小cap行为统一留到§5.1–5.2先RED。
- [x] 4.2 **RED（只写测试）**：有效调用的child首个请求messages恰为固定System+task，使用调用时runtime snapshot、固定Normal mode、`Depth::Low`和`Passthrough`；不含parent history/thinking/permission mode/plan transient instruction/session。再用Barrier控制同一child两轮：首轮后pair-replace parent runtime，当前child第二轮仍用旧tuple，下一次新delegate才用新tuple。
- [x] 4.3 **RED（只写测试）**：child registry/schema/capability恰含`list_dir/read_file/glob/grep + ReadOnly`并保持root顺序；模型硬发web/write/edit/shell/ask/plan/update/delegate时，scope/lookup/gate fail-closed且decider/preview/UI/execute均为0。
- [x] 4.4 **RED（只写测试）**：产品root depth=1可见delegate并派生child depth=0；有效参数后立即capture invocation time/snapshot，child budget为`min(parent,8)`与`min(parent deadline, invocation_time+120s)`且deadline覆盖进程级blocking permit等待与root canonicalization preflight；preflight与四fs必须共享同一进程级limiter。用两波受控closure验证首批4个awaiting future被取消后permit仍随closure持有，旧closure释放前新preflight/fs不得entered且global max-active≤4。legacy root/wrappers仍depth=0，schema隐藏delegate且硬发既有scope error，在observer/permission/execute前fail-closed。
- [x] 4.5 **用户确认停点**：贴出§4.1–4.4测试代码与原始RED输出；这是新内置工具与派生路径首次成型，等待明确批准后才能进入GREEN。
- [x] 4.6 **GREEN**：实现DelegateTaskTool、固定system prompt、内部deny decider、restricted registry与显式read-root child Agent factory；严格按“校验参数→capture invocation time/runtime snapshot→派生child scope→在child termination等待下取得进程级blocking permit→把permit移入`spawn_blocking` closure canonicalize root→从不可变snapshot构造并运行child”执行，使参数/history/schema/capability/budget测试全绿；permit等待与join均受scope终止约束，已启动closure持有permit到真实结束。不阻塞async runtime thread，不加入config、第三方SDK或递归入口。
- [x] 4.7 **GREEN/assembly**：先创建共享runtime与root基础registry、派生四工具view，再用同一runtime Arc注册delegate及可选交互工具，最后经`Agent::with_runtime`构造parent；缺预期工具时fail-fast。锁parent/delegate runtime identity并更新assembled registry/tool count测试，`default_registry`既有契约按code事实最小调整。

## 5. Child结果、failure与termination收口（强制TDD）

- [x] 5.1 **RED（只写测试）**：cap足够时成功raw结果精确为`subagent report (untrusted):\n{非空final text}`、`is_error=false`、`exit=None`；parent仍active时的空final、unscoped/args/root/derive/Provider/Agent/child-only deadline ordinary error先形成稳定`delegate_task failed: {reason}`raw envelope。depth dispatch拒绝保持scope error，parent cancel/deadline只生成synthetic termination且不含delegate前缀；所有分支均不得泄漏history/thinking/未发布outcome。
- [x] 5.2 **RED（只写测试）**：success/error raw envelope超过`max_output_bytes`时，ASCII/中日韩/emoji均在UTF-8边界整体截断并正确置`truncated`；cap小于固定前缀自身时content只能是对应raw envelope的有效前缀片段、长度不超cap，success不得越过前缀泄漏child文本且不panic。
- [x] 5.3 **RED（只写测试）**：用`tokio::time::pause/advance`和可控stalled canonicalizer锁child 120秒deadline覆盖permit等待与preflight、只形成outer ordinary is_error ToolResult、Provider为0且parent继续下一轮；更早parent deadline/cancel产生唯一synthetic termination，无`delegate_task failed:`或普通child error重复发布。重跑§4.4跨波次limiter用例作为regression，不把已由§4.6实现的permit retention伪装成本节新RED。
- [x] 5.4 **GREEN**：实现Noop child DeltaSink、raw envelope后统一UTF-8 truncation、精确error mapping、scoped preflight和child-only deadline处理，使§5.1–5.3全绿；不得直接写parent history或把parent termination吞成ordinary ToolOutcome。
- [x] 5.5 **regression**：增加delegate端到端静态workspace escape fixture，断言外部marker不进child ToolResult/history或后续Provider请求；重跑Provider error、max_iterations/forced-final、scope cancellation与duplicate occurrence全组，确认child failure不改变既有AgentError/ScopedAgentError分类。

## 6. 有界并行regression、observer与迟到结果（observer/cancellation强制TDD）

- [x] 6.1 **regression**：用oneshot/Barrier让5个连续delegate保持受控，锁前4个可同时进入、第5个等待、峰值≤4；释放顺序乱序时outer result/finished/history仍按occurrence，重复call id各一结果。该行为由§4的`ParallelSafe + ReadOnly`接入既有scheduler后继承，不要求伪造RED。
- [x] 6.2 **regression**：锁`read_file, delegate_task×N, run_shell`中前两类进入同一eligible segment、outer active合计≤4，`run_shell`必须等待该segment全部按occurrence发布后启动；child Provider future不计入process blocking limiter，workspace preflight与child内部四读取共用该进程级上限4。该用例验证既有batch/barrier组合，不作为新scheduler RED。
- [x] 6.3 **RED（只写测试）**：parent在child Provider、child parallel read及第5个等待slot三处取消；再用确定性race Tool让outer termination branch首次poll为Pending、tool future随后同步cancel scope并立即ready。另用Barrier让ParallelSafe后项先ready进入私有buffer、前项保持未完成，再取消parent。全部future必须收口；串行outcome与每个parallel连续可发布item都在紧邻history/finished且中间无`await`的位置经post-ready checkpoint，私有buffer不算发布，未发布ordinary outcome全部丢弃，无迟到text/status/tool-finished/usage/Idle，下一turn可继续。
- [x] 6.4 **RED（只写测试）**：通用scoped observer收到可归属的child identity/status/tool/usage；child触顶forced-final时，同一identity在请求前收到`CallingModel`，带usage响应后恰上送该usage，成功时再发送`Idle`，而Provider error/termination/空final不发`Idle`；ChannelObserver只转发root status/tool、聚合普通与forced-final child usage，child call id不得收口outer卡，逻辑事件恰好一次。
- [x] 6.5 **GREEN**：接通delegate observer传递与ChannelObserver identity过滤；修复Agent forced-final `CallingModel`/usage/Idle；serial结果及parallel ready buffer中每个连续可发布item都必须在紧邻同步history/finished发布且中间无`await`的位置重查parent scope，使§6.1–6.4全绿。不得把入buffer当作publication linearization point，不得新增第二套scheduler或可配置并发器。
- [x] 6.6 **flaky guard**：多次运行并发/取消targeted测试，全部使用ack/Barrier/虚拟时间而非sleep；失败清理必须release sender并abort driver，不能遗留后台child。

## 7. 产品入口、TUI与session集成（TUI事后回归）

- [x] 7.1 TUI每个Prompt改用显式depth=1产品root；headless改用相同scoped入口并保持stdout/error/exit既有契约。legacy library `run/run_observed/root_scope`继续depth=0。
- [x] 7.2 增加assembled TUI/headless集成测试：四模式可见且零权限框、provider picker/session restore经pair-replace后child只观察完整新Provider/model tuple、单`/model`保持same-provider/new-model语义、child-only deadline不结束parent、parent Interrupt只产生一个terminal event。
- [x] 7.3 对照`设计规范/03-组件清单.md` C5/C10，用TestBackend+insta新增Midnight/Daylight delegate running/success/error/truncated快照；首次新快照交用户对`设计规范/原型截图/`人工审查后才能accept，现有非delegate快照必须零churn且不得遗留`.snap.new`。
- [x] 7.4 session测试锁只保存outer delegate ToolCall/ToolResult/ToolCard；child history/identity/scope/内部卡不入JSONL，Interrupt后`--continue`与picker `--resume`无Running残留、不重跑child且首轮Provider正常。
- [x] 7.5 复核TUI queue、permission modal Esc、Plan进度、token统计、provider picker/model switch与普通工具卡全组，确认child普通与forced-final usage均计入turn但child stream/内部卡不进入transcript。

## 8. 文档、自动化门禁与审查

- [x] 8.1 更新`技术方案/mysteries-agent技术方案.md`、README与CHANGELOG `[Unreleased]`：描述单层只读delegate、同时active outer child≤4（不是调用总量上限）、每child最多8个tool-enabled调用加1次forced-final、120秒总wall-clock、workspace confinement与`delegate occurrence数 × 最多9`的成本放大；如实声明无递归/后台/child session/token总预算/写与网络child，并明确per-response occurrence与child-only扫描字节仍无硬上限、四fs工具先读取/遍历/收集且仅`read_file`/`grep`按输出cap后置截断的既有语义。新增`manual-verification.md`，给出可复制的PowerShell stall Provider与双marker OpenAI-compatible fixture启动、配置、日志oracle和清理命令。
- [x] 8.2 审查`Cargo.toml`/`Cargo.lock`无变化且无新增dependency；核对config、CLI grammar、Provider wire、PermissionMode矩阵、session JSONL、ToolOutcome结构与existing snapshot metadata均未改变。
- [x] 8.3 **targeted**：scoped Tool/runtime pair replace与frozen snapshot、四fs containment（含ignore metadata边界）、preflight/fs共享blocking limiter、delegate schema/history/capability/budget/result、provider switch、parallel/post-ready cancel/deadline/forced-final `CallingModel`/usage/Idle observer、TUI/headless/session全组稳定全绿。
- [x] 8.4 **format + full Rust**：运行`cargo fmt --all -- --check`；随后在每个独立PowerShell shell设置`$env:CARGO_TARGET_DIR='target/codex-readonly-subagent'`，依次运行`cargo clippy --all-targets --locked -- -D warnings`、`cargo test --locked`、`cargo build --release --locked`。隔离target仍被锁时报告并停止，不得kill进程。
- [x] 8.5 **security/OpenSpec**：运行`cargo-audit audit --deny unsound --file Cargo.lock`、`openspec validate add-readonly-subagent --strict`与`openspec validate --all --strict`，区分允许warning与0 vulnerability/0 unsound结论。
- [x] 8.6 **范围/快照**：运行`git diff --check`、`git diff --name-only`、`git ls-files --others --exclude-standard`，逐文件证明只改本change生产/测试/文档；不得修改`.ai_history`（只在archive且用户审阅后写）、不得遗留`.snap.new`或无关fmt churn。
- [x] 8.7 **对抗式审查**：重点检查runtime陈旧/pair撕裂/运行中child迁移、scope串线、unscoped delegate绕过、ReadOnly误含交互工具、workspace preflight detached closure与absolute/`..`/symlink/junction逃逸、parent/nested ignore precedence及显式ignored target不得被提前剪枝、buffered-ready/post-ready parent cancel与child deadline竞态、并发上限被误当总量、重复id、forced-final `CallingModel`/usage/Idle、observer/TUI污染、child状态持久化与极小cap envelope；修复后重跑受影响targeted及完整门禁。

## 9. 真机核验（用户专属；实施Agent不得代勾）

- [x] 9.1 TUI在Normal与Plan分别要求“委派child只读分析指定模块并给路径/行号”，确认出现一张`delegate_task`卡、无权限框、无child内部卡，最终报告可核验且普通下一轮正常。
- [x] 9.2 要求同一轮委派5个互不依赖的只读任务；UI允许5张outer卡先进入Running（表示已调度），只人工确认最终5份结果按请求顺序收口且界面不卡死；物理active child峰值≤4只以§6.1 Barrier自动测试为oracle。
- [x] 9.3 按`manual-verification.md`启动本地stall OpenAI-compatible fixture并让child停在可观察Provider等待点后按Esc，确认仅一次“已中断本轮”、outer卡Error、无迟到Done/text/status，立即提交新Prompt可完成；不得再用真实Provider响应速度测试手速。
- [x] 9.4 在测试workspace外准备无敏感内容的marker文件，并让child分别尝试absolute、`..`及可用时junction路径读取；三者均应拒绝且报告中不出现marker，root普通读取既有行为另行确认未变。另按`manual-verification.md`证明read root外parent ignore不影响child；可创建file symlink时，workspace内linked `.gitignore`必须在解析前以containment error拒绝且外部规则marker不泄漏，否则记录原始OS skip。
- [x] 9.5 按`manual-verification.md`启动两个返回不同marker并记录request.model的本地OpenAI-compatible fixture；provider picker成对切换后再委派，以日志证明child只看到完整新Provider/model tuple；另用`/model`证明same-provider/new-model单字段语义。中断/成功后退出，分别用`--continue`与picker `--resume`确认无child残留或自动重跑。

## 10. Archive checklist（不计入apply进度；不得改为checkbox）

仅在全部checkbox完成、用户完成真机与快照核验并明确发起archive后执行：

1. 确认artifacts 4/4 complete，`openspec instructions apply --change add-readonly-subagent --json`为remaining=0。
2. 展示`readonly-subagent`新增spec及`agent-execution-scope`、`agent-loop`、`builtin-tools`、`tool-system`、`tui-shell`五份delta sync摘要并取得用户选择；sync后同步更新六个capability的Purpose，尤其把`builtin-tools`从12个更新为13个并纳入`delegate_task`，同时让`agent-loop` Purpose反映forced-final `CallingModel`/usage/Idle与publication linearization契约。
3. 执行archive move；按实际archive目录数量更新README，不提前写预测值。
4. 按AGENTS.md起草本change的`.ai_history/logs/...` archive决策记录，交用户审阅，并与archive进入同一提交。
5. 运行`openspec validate --all --strict`、`git diff --check`和archive路径/数量/主spec Purpose/决策记录复核。
