## ADDED Requirements

### Requirement: delegate_task元数据与参数契约固定

系统 SHALL 注册名为`delegate_task`的内置工具，description明确它只执行独立workspace只读调研并返回untrusted报告；JSON schema只允许必填非空字符串`task`且拒绝额外字段。其permission level MUST 为`ReadOnly`、concurrency MUST 为`ParallelSafe`、`plan_only` MUST 为false、required child depth MUST 为1，Network preview保持不可授权默认值。因legacy `Tool::execute`缺少scope与observer，直接调用该入口 MUST 稳定fail-closed且零child/Provider副作用；真实委派只允许经`execute_scoped`进入。

#### Scenario: 元数据逐项锁定
- **WHEN**查询assembled root registry中的`delegate_task`
- **THEN**名称、schema、description、ReadOnly、ParallelSafe、非plan-only、required child depth 1与不可授权Network preview均符合固定契约

#### Scenario: malformed args零副作用
- **WHEN**args为null、缺task、task类型错误、空白task或含额外字段
- **THEN**outer observer started/finished仍各一次且工具返回is_error，但child scope派生、child observer、Provider与四个读取工具调用次数均为0

#### Scenario: unscoped execute稳定拒绝
- **WHEN**调用方绕过Agent Loop直接调用`DelegateTaskTool::execute`
- **THEN**返回以`delegate_task failed: scoped execution context required`形成的bounded is_error结果，不panic、不自造root scope，child scope、observer、Provider与fs工具调用次数均为0

### Requirement: 四个读取工具执行child-only containment

`list_dir`、`read_file`、`glob`与`grep` SHALL 在scoped execution context带read root时，把canonical containment检查和实际读取/遍历放进同一个既有blocking worker/permit；worker MUST 先验证canonical target，再直接使用该target执行I/O，不得重新解析原始输入。目录walker为匹配规则读取的parent、`.ignore`与`.gitignore` metadata/content同样属于受控I/O：parent discovery MUST 在canonical read root停止，每个可能加载的规则文件 MUST 在打开、解析前canonicalize并验证仍位于read root。read root为None时 SHALL 直接复用既有execute路径。错误 MUST 编码为ToolOutcome而非panic。

#### Scenario: containment在blocking worker内且实际I/O前拒绝
- **WHEN**child对workspace外目标调用任一四读取工具
- **THEN**调用取得既有blocking permit后、目标content read或directory traversal前返回is_error，对应内容读取/遍历helper计数为0且permit正常释放；canonicalization为解析路径所需的metadata或OS handle访问允许，不得把它误计为内容泄漏

#### Scenario: canonical allowed target进入既有语义
- **WHEN**三个目录工具的canonical target为read root本身或其descendant目录，或`read_file` target为root下canonical file
- **THEN**工具进入既有execute逻辑，gitignore、truncation、ParallelSafe分类与结果逐字段不变；`read_file`传root目录时通过containment后仍返回既有directory-read error

#### Scenario: 链接解析后越界
- **WHEN**表面路径位于workspace内但canonical target经symlink或junction落在外部
- **THEN**四工具均fail-closed，不得按表面字符串前缀放行

#### Scenario: 外部parent ignore规则不跨越read root
- **WHEN**canonical read root的ancestor含会隐藏workspace内probe的`.ignore`或`.gitignore`
- **THEN**scoped `list_dir` / `glob` / `grep`不得打开或解析该规则，probe仍可见；相同root工具在`read_root=None`时继续保持既有parent规则语义

#### Scenario: linked ignore规则在解析前越界拒绝
- **WHEN**workspace内`.ignore`或嵌套`.gitignore`经symlink canonical到read root外的规则文件
- **THEN**scoped目录工具在actual target walk前返回is_error，外部规则marker不得进入ToolOutcome、child history或后续Provider请求；workspace内普通parent与嵌套规则继续生效

## MODIFIED Requirements

### Requirement: 内置工具并发分类与异步运行时隔离

13 个内置工具 SHALL 具有完整、固定的 `ToolConcurrency` 分类：`list_dir` / `read_file` / `glob` / `grep` / `delegate_task` MUST 为 `ParallelSafe`；`web_fetch` / `web_search` / `write_file` / `edit_file` / `run_shell` / `submit_plan` / `update_plan` / `ask_user` MUST 为 `Exclusive`。该分类不得由 `PermissionLevel` 推导；尤其三个交互 / 计划工具虽为 `ReadOnly`，仍必须独占。`delegate_task`虽为`ReadOnly`，其并发安全还依赖独立child history/scope/runtime snapshot，不能推广为“ReadOnly自动并行”。

四个 `ParallelSafe` 本地读取工具的同步文件读取、目录遍历与内容扫描工作，以及`delegate_task`的workspace-root canonicalization preflight，MUST NOT 在调用方 Tokio worker 上直接阻塞；它们 SHALL 把同步主体 offload 到 blocking worker，async路径只等待结果。生产执行 SHALL 共用同一个进程级 blocking limiter（上限 4）：调用取得 permit 后 MUST 把 permit 移入 blocking closure并持有到真实同步工作结束，使调用 future / JoinHandle 被 drop 后旧 closure、preflight与新turn文件工作合计仍不超过4。preflight等待permit和JoinHandle时还 MUST 受child scope cancellation/deadline约束。blocking worker无法join（含worker panic）时 MUST 返回普通`is_error`工具结果，不得panic；既有permission level、schema、gitignore、排序、分页、截断、错误文案与`exit=None`契约 MUST 保持不变。

#### Scenario: 五个只读工具显式 ParallelSafe

- **WHEN** 查询 `list_dir` / `read_file` / `glob` / `grep` / `delegate_task` 的 `concurrency()`
- **THEN** 五者均返回 `ParallelSafe`且`permission_level()`均为`ReadOnly`；delegate不得新增第二个blocking limiter，其workspace preflight与四fs工具共享既有进程级实例

#### Scenario: 其余八个工具完整锁定 Exclusive

- **WHEN** 查询 `web_fetch` / `web_search` / `write_file` / `edit_file` / `run_shell` / `submit_plan` / `update_plan` / `ask_user` 的 `concurrency()`
- **THEN** 八者均返回 `Exclusive`；Network、Edit、Execute 与三个 `ReadOnly` 交互 / 计划工具都没有并行 opt-in

#### Scenario: 阻塞文件工作不占住调用方 worker

- **WHEN** 在 current-thread 测试 runtime 中让一个经同一 blocking helper 调度的受控文件工作发出 entered 后等待 std release，同时调度另一个独立 async probe，并由外部 OS watchdog 保证失败路径也会 release
- **THEN** async probe 的 ack 先于 release 被观察到，证明同步主体已离开调用方 Tokio worker；测试不得依赖同一 Tokio worker 上的 timeout 或 sleep 时长解死锁

#### Scenario: Interrupt 后新 turn 不突破进程级 blocking 上限

- **WHEN** 首批 4 个 blocking closure 已 entered且其awaiting futures被取消，随即从新turn再提交4个读取工作
- **THEN** 旧closure结束并释放permit前新closure不得entered，跨preflight与读取两类工作记录的global max-active始终≤4；测试使用独立limiter与per-call ack，不污染并行测试

#### Scenario: offload 前后工具行为零回归

- **WHEN** 对四个本地读取工具复跑既有正常、失败、排序、分页、gitignore与UTF-8截断测试
- **THEN** root read root为None时`ToolOutcome`与change前逐字段一致；child read root存在时只增加同一blocking worker内的canonical containment

#### Scenario: blocking worker join failure 编码为工具错误

- **WHEN** 以测试seam令blocking helper panic或返回JoinError
- **THEN** `execute`返回`is_error=true`且包含稳定的worker failure说明，Agent进程不panic
