## ADDED Requirements

### Requirement: 内置工具并发分类与异步运行时隔离

12 个内置工具 SHALL 具有完整、固定的 `ToolConcurrency` 分类：`list_dir` / `read_file` / `glob` / `grep` MUST 为 `ParallelSafe`；`web_fetch` / `web_search` / `write_file` / `edit_file` / `run_shell` / `submit_plan` / `update_plan` / `ask_user` MUST 为 `Exclusive`。该分类不得由 `PermissionLevel` 推导；尤其三个交互 / 计划工具虽为 `ReadOnly`，仍必须独占。

四个 `ParallelSafe` 本地读取工具的同步文件读取、目录遍历与内容扫描工作 MUST NOT 在调用方 Tokio worker 上直接阻塞；它们 SHALL 把同步主体 offload 到 blocking worker，async `execute` 只等待结果。生产执行 SHALL 共用进程级 blocking limiter（上限 4）：调用取得 permit 后 MUST 把 permit 移入 blocking closure 并持有到真实同步工作结束，使调用 future / JoinHandle 被 drop 后旧 closure 与新 turn 的 blocking 工作合计仍不超过 4。blocking worker 无法 join（含 worker panic）时 MUST 返回 `ToolOutcome{is_error:true}`，不得 panic；既有 permission level、schema、gitignore、排序、分页、截断、错误文案与 `exit=None` 契约 MUST 保持不变。

#### Scenario: 四个本地读取工具显式 ParallelSafe

- **WHEN** 查询 `list_dir` / `read_file` / `glob` / `grep` 的 `concurrency()`
- **THEN** 四者均返回 `ParallelSafe`，且 `permission_level()` 仍为 `ReadOnly`

#### Scenario: 其余八个工具完整锁定 Exclusive

- **WHEN** 查询 `web_fetch` / `web_search` / `write_file` / `edit_file` / `run_shell` / `submit_plan` / `update_plan` / `ask_user` 的 `concurrency()`
- **THEN** 八者均返回 `Exclusive`；Network、Edit、Execute 与三个 `ReadOnly` 交互 / 计划工具都没有并行 opt-in

#### Scenario: 阻塞文件工作不占住调用方 worker

- **WHEN** 在 current-thread 测试 runtime 中让一个经同一 blocking helper 调度的受控文件工作发出 entered 后等待 std release，同时调度另一个独立 async probe，并由外部 OS watchdog 保证失败路径也会 release
- **THEN** async probe 的 ack 先于 release 被观察到，证明同步主体已离开调用方 Tokio worker；测试不得依赖同一 Tokio worker 上的 timeout 或 sleep 时长解死锁

#### Scenario: Interrupt 后新 turn 不突破进程级 blocking 上限

- **WHEN** 首批 4 个 blocking closure 已 entered 且其 awaiting futures 被取消，随即从新 turn 再提交 4 个读取工作
- **THEN** 旧 closure 结束并释放 permit 前新 closure 不得 entered，跨两批记录的 global max-active 始终 ≤4；测试使用独立 limiter 与 per-call ack，不污染并行测试

#### Scenario: offload 前后工具行为零回归

- **WHEN** 对四个本地读取工具复跑既有正常、失败、排序、分页、gitignore 与 UTF-8 截断测试
- **THEN** `ToolOutcome` 与 change 前逐字段一致；仅执行线程位置和并发分类改变

#### Scenario: blocking worker join failure 编码为工具错误

- **WHEN** 以测试 seam 令 blocking helper panic 或返回 JoinError
- **THEN** `execute` 返回 `is_error=true` 且包含稳定的 worker failure 说明，Agent 进程不 panic
