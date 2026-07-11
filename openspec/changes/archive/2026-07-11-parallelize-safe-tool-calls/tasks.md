# Tasks — parallelize-safe-tool-calls

执行边界：本 change 只让 `list_dir` / `read_file` / `glob` / `grep` 以固定上限 4 并行；Network、Edit、Execute、Plan / 交互工具保持 `Exclusive`。不新增 crate / config / session wire，不跨屏障重排，不实现通用 Agent cancellation、MCP / subagent。headless `Tool` / Agent Loop / Interrupt history 强制 RED→GREEN；为避免新类型造成编译红，接口 scaffold 与 RED 测试必须拆成不同 checkbox，RED 步只写测试。TUI 外壳走接线后 `TestBackend + insta`，不得以改旧快照掩盖回归。实施期间不得执行 git 写操作、不得 kill 用户进程；Cargo 使用隔离的 `CARGO_TARGET_DIR=target/codex-parallel-tools`，若该隔离 target 仍被锁则报告并停止，不得换回默认 target 规避。

## 1. 并发元数据与 12 工具分类（强制 TDD；接口停点）

- [x] 1.1 **baseline**：在无代码改动时运行 `$env:CARGO_TARGET_DIR='target/codex-parallel-tools'; cargo test --lib --locked`，记录当前通过数；确认 `git diff --name-only` 只含本 change 规划文件。
- [x] 1.2 **interface scaffold（不写测试）**：在 `src/tool/mod.rs` 只落可编译 `ToolConcurrency::{Exclusive, ParallelSafe}` 与 `Tool::concurrency()` default `Exclusive`，机械补 imports / mock 字段但不为任何生产工具 opt-in；运行 `cargo test --lib --no-run --locked` 证明 scaffold 编译，不宣称目标行为完成。
- [x] 1.3 **RED（停点；只写测试）**：锁 default mock=`Exclusive`、显式 mock 分类可经 registry 查询，并用带 Mock `PlanApprover` / `UserPrompter` / `PlanProgressReporter` 的专用 12-tool fixture（不得误用只有 9 个基础工具的 `default_registry()`）断言四个本地读取=`ParallelSafe`、其余八个=`Exclusive`。运行 targeted tests，确认至少四个本地读取分类以**断言失败**落红，非编译错、非 panic / `todo!()`。
- [x] 1.4 **用户确认停点**：贴出 §1.3 测试代码与原始 RED 输出，等待用户明确批准后才能进入 GREEN。
- [x] 1.5 **GREEN**：仅让 `ListDirTool` / `ReadFileTool` / `GlobTool` / `GrepTool` override `concurrency()==ParallelSafe`，其余工具依 default `Exclusive`；使 §1.3 全绿，不改 permission level、schema 或 execute 行为。
- [x] 1.6 **regression**：重跑 `ToolRegistry` 注册 / 重名 / schemas / `schemas_for(Plan)`、四级 `PermissionLevel` 与 12 工具 schema 顺序测试，证明并发元数据不参与权限和 schema 过滤。

## 2. 本地读取的 blocking worker 与进程级上限（强制 TDD）

- [x] 2.1 **helper scaffold（不写测试、不接生产工具）**：只落可测试 `BlockingToolLimiter` / `run_blocking_tool` 签名、test-injected limiter 与 production accessor；scaffold 可用无上限 `spawn_blocking` 且 JoinError 仍传播/占位，使代码可编译但明确不满足 cap / error 契约。运行 `cargo test --lib --no-run --locked`，四个生产文件工具仍走旧路径。
- [x] 2.2 **RED（只写测试）**：测试 closure thread 与调用方不同；在 `#[tokio::test(flavor="current_thread")]` 中让 closure 发 entered 后等 std release、独立 async probe 发 ack，并由外部 OS watchdog 在失败路径也 release，断言 probe 先于 release（不得依赖同 worker timeout 解死锁）；另以专用 limiter 锁 8 个受控 closure 的 max-active≤4、drop 首批 awaiting futures 后第二批仍不能越过旧 permit，以及 worker panic→is_error。运行 targeted tests，确认至少 cap / join-error 断言红。
- [x] 2.3 **GREEN**：用 `tokio::task::spawn_blocking` + `Arc<Semaphore>(4)` 实现 limiter；先 acquire owned permit，再把 permit move 进 closure 直到真实完成；正常 join 返回原 outcome，JoinError 映射稳定 `ToolOutcome{is_error:true}`，使 §2.2 全绿。生产使用进程共享 limiter，测试使用独立实例避免并行测试串扰。
- [x] 2.4 将 `list_dir` / `read_file` / `glob` / `grep` 的现有同步主体分别抽成接收 owned args / cloned `ToolContext` 的 helper，并让 async `execute` 经 §2.3 offload；不得改原算法、排序、gitignore、分页、UTF-8 截断或错误文案。
- [x] 2.5 **characterization**：复跑 `src/tool/fs.rs` 全部既有正常 / 缺失 / 非法 pattern / CJK 截断测试，逐字段锁定 `content/is_error/truncated/exit` 零回归；线程隔离只在 generic helper 的 deterministic liveness test 锁定，四个 Tool 以共享 helper callsite + 行为测试证明接线，不新增 injectable runner seam。

## 3. 连续安全批次、work-conserving 补位与顺序（强制 TDD；调度路径停点）

- [x] 3.1 **status scaffold（不写测试）**：只为编译增加 `AgentStatus::ExecutingTools(usize)`；对现有 exhaustive match 做机械 fallback（不得实现目标文案 / 调度），Agent Loop 仍完全串行且不发送该 variant。运行 `cargo test --lib --no-run --locked`。
- [x] 3.2 **RED（停点；只写测试）**：在 `src/agent/mod.rs` 增受控 `ParallelSafe` Mock Tool，每个 occurrence 预建独立 entered / release / completed oneshot 并配 active/max-active 原子计数；测试两个安全调用在 release 前 active=2、5 调用 max-active=4、`[safe-1,safe-2,exclusive-3,safe-4]` 不跨屏障、多项发 `ExecutingTools(total)` / 单项仍 `ExecutingTool(name)`、误标 ParallelSafe 的 Network 被 clamp。timeout 只用 `start_paused` 虚拟时钟作 watchdog；失败前必须 release 或 abort driver，禁止 Barrier / Notify / sleep 判并发。
- [x] 3.3 **RED（同一停点；只写测试）**：保持 call-1 未 release、让 call-2 completed，断言 call-5 在 call-1 release 前 entered（work-conserving）；用独立 release 控制 `[call-1,call-2]` 物理逆序完成但 history / finished / Provider messages 仍按 occurrence；另锁单项 error 不取消兄弟、`[safe,unknown,safe]` 屏障、`ParallelSafe+ReadOnly+plan_only` 屏障，以及同批两个 occurrence 复用 id 仍产两份结果。当前串行实现应以重叠 / 调用数 / status 断言落红。
- [x] 3.4 **用户确认停点**：贴出 §3.2–3.3 测试代码与原始 RED 输出，等待用户明确批准后才能实现调度 GREEN。
- [x] 3.5 **GREEN**：在 Agent Loop 抽取单项 prepare / outcome 收口逻辑；按 provider 顺序识别最大连续 eligible 段（Tool 存在 + `ParallelSafe` + `ReadOnly` + `!plan_only`），用现有 `futures-util::StreamExt::buffer_unordered(MAX_PARALLEL_TOOL_CALLS)` 且常量=4 执行，future 返回 `(original_index,outcome)`；其他调用走原串行路径并形成屏障，不把 registry 迁为 `Arc`、不新增依赖。
- [x] 3.6 **GREEN**：以 index-keyed ready buffer + `next_publish` 只发布连续 ready 前缀；每个 occurrence 依次 `history.push` 再 `on_tool_call_finished`，重复 id 不去重。长度 >1 在执行前按模型顺序发整段 started + `ExecutingTools(total)`，长度=1 保持 `ExecutingTool(name)`；窗口补位不重复发 status。使 §3.2–3.3 全绿。
- [x] 3.7 **regression**：锁 unknown tool、Plan 纵深拒、非 Plan plan_only 拒绝、ReadOnly 直放、Network / Edit / Execute gate 与 `max_iterations` forced-final 路径行为不变。

## 4. 调度收口、observer 与全量回归

- [x] 4.1 重跑 §3 全组 targeted tests 多次，确认 overlap / per-Agent limit / work-conserving refill / 三类 barrier / clamp / reverse-order / duplicate-id occurrence / error-isolation / provider-wait 均稳定全绿、无依赖 sleep 的 flaky 判据。
- [x] 4.2 在全绿前提下清理原串行与批次分支的 outcome / history / observer 重复代码，保持每个 error content 与调用顺序不变；重跑 §3 tests 防行为漂移。
- [x] 4.3 扩展 `RecordingObserver` 回归：整段 started（模型 occurrence 顺序）→ `ExecutingTools(total)` → finished（模型 occurrence 顺序）；重复 id 产生对应次数 finished。单工具、permission denial、Network `readonly=false`、usage 与 no-op observer 既有事件序列零回归。
- [x] 4.4 复跑 agent-loop 全量单测，额外断言每轮第二次 `provider.complete` 只在该 Assistant 的全部 tool-call occurrence 均已有一个 ToolResult 后发生。

## 5. Channel 与 TUI 并行状态接线（TUI 外壳；事后测试）

- [x] 5.1 在 TUI `Phase` 正式接通 `ExecutingTools(usize)`，替换 §3.1 的 mechanical fallback；`ChannelObserver` 继续经同一 `mpsc` forward，整段多 started / 单 status / 有序 finished 不新建 channel，不改 `ToolCard` 字段或 session JSONL。
- [x] 5.2 给 `AppState.apply` 补 reducer 测试：`ExecutingTools(2)` / `(5)` 映射为保留 batch total 的 Phase；五个 started 可按到达顺序产生五张 Running（含等待窗口）；finished 查找**最早同 id 且仍 Running**的卡。覆盖旧 Done 卡 + 两张同 id Running 卡依次收口，transcript 顺序 / 旧卡不变；单 started 路径零回归。
- [x] 5.3 按 `设计规范/02-布局与交互.md` 状态机、`设计规范/03-组件清单.md` C5 / C10 接入活动状态行：2≤N≤4 显示 `并行执行 N 个工具…`，N>4 显示 `处理 N 个工具（最多并行 4）…`，均带既有 spinner / Esc hint；port C5 已调度 Running，adapt C10 总数 / 上限文案，drop 新面板 / queued 持久化状态 / per-tool spinner；不新增 theme token。

## 6. Interrupt history 与 Running 卡收口（强制 TDD；新取消路径停点）

- [x] 6.1 **activation seam scaffold（不写新行为测试、不做 normalization）**：抽取 `load_session_for_activation(store, id)`（或等价可单测 seam），让 `prepare_session_startup` 的 `--continue` 分支与 picker 选中后的 `--resume` hot-swap branch 都经该 seam 取得 tuple；同时只落可编译的 `normalize_loaded_session(&mut history, &mut transcript)` no-op 签名并由 activation seam 调用，不得 `todo!()` / panic。两处调用各自沿用现有 System replacement / provider / plan / state assignment。运行 session / startup 既有测试与 `cargo test --lib --no-run --locked`，证明 raw round-trip、磁盘与运行行为零变化。
- [x] 6.2 **RED（停点；只写测试）**：经现有 `run_agent_task` + 可控 Mock Provider / per-call entered-release oneshot 写失败集成测试：本 turn 两个未完成 occurrence 中断后应补 interrupted；已完成前缀不重复；更早 turn 不改；较早 Assistant 的 `call-1` 已配对、较后 Assistant 复用 `call-1` 后中断时仍须补后一 occurrence；无 tool_calls 时零变化。再用真实旧 session fixture 分别测试 §6.1 的 picker activation seam 与 `prepare_session_startup(--continue)`：一个 Assistant 组含两个同 id occurrence 但只有首项结果，transcript 含多张 Running、既有 Done / Error 与其他 block；直接对 no-op normalization scaffold 调用两次，锁期望的 history 补齐、Running 字段重置、其余逐字段不变与幂等；另锁首次 RecordingProvider 无 dangling、新 turn 同 id finished 只更新新卡。当前实现必须以 missing ToolResult / stale Running 的**断言失败**落红，非编译错；失败清理 release / abort driver。
- [x] 6.3 **用户确认停点**：贴出 §6.2 全部测试代码与原始 RED 输出，等待用户明确批准后才能进入 GREEN。
- [x] 6.4 **GREEN**：新增可复用的 occurrence / FIFO matcher；当前 turn 路径由 `complete_interrupted_tool_results(history, turn_history_start)` 扫描 suffix，ToolResult 消费同 id 最早未配对 occurrence，最后按原 occurrence 顺序补 interrupted。`run_agent_task` 在当前 User 入 working 后记 checkpoint，interrupt arm drop run future、补齐并保存，再只发既有 `Interrupted`，不得发合成 finished / Idle。
- [x] 6.5 **run-agent integration**：两个安全 Mock Tool entered 后投入 Interrupt；用 `start_paused` watchdog / `try_recv` 锁只发 Interrupted、无 trailing finished / Idle、agent task 存活，`input_rx` 中直接 send 的后续 Prompt 不被 interrupt arm 吞掉。此项不借 channel backlog 推断 AppState queue 行为。
- [x] 6.6 `AppState.apply(Interrupted)` 将所有仍 Running 的 C5 卡改 Error、output=`工具调用已中断`，已 Done / Error 卡不改，再追加既有 notice；单独复用 / 扩展 `interrupted_terminal_event_still_advances_queue` 锁 UI queue gate 只 dequeue 一条，并保持 pending permission / plan / question 清理语义。
- [x] 6.7 **blocking cancellation + global cap characterization**：首批 4 个无副作用 blocking closure entered 后 drop awaiting batch，立即提交第二批；用 closure-completed ack 证明旧 closure 可自然结束、迟到 outcome 不触发 finished / history，同时共享 permit 使两批 global max-active≤4、新批须等旧 closure release。失败路径必须释放所有 closure 并 abort driver。
- [x] 6.8 **GREEN · loaded session 双收口**：以 §6.4 matcher 替换 §6.1 的 no-op `normalize_loaded_session` body，使两处实际 load site 同时收口。history 逐 Assistant 结果组消费 occurrence，在下一个非 ToolResult 前为剩余项补 interrupted，不删除 / 改写已有结果、不用 id 去重；transcript 将全部 Running 改 Error、output=`上次会话已中断`、`truncated=false`、`exit=None`，其余 block 不变。raw `SessionStore::load` 与磁盘不变；使 §6.2 的 picker seam / continue wiring、pure-state、幂等与首次 Provider 测试全部转绿，不在本步新增测试行为。

## 7. TUI 快照与用户视觉停点

- [x] 7.1 生成但**不 accept** 八份新 `.snap.new`：Midnight + Daylight 的“两张同时 Running C5 卡 + `ExecutingTools(2)`”；Midnight + Daylight 的“五张已调度 Running + `ExecutingTools(5)` / 最多并行 4 文案”；Midnight + Daylight 的“Interrupted 后两卡 Error + notice”；Midnight + Daylight 的“旧 session Running 已规范化为 Error / 上次会话已中断，且新 turn 同 id 卡正常 Done”。固定 spinner frame，保证可复现。
- [x] 7.2 对照 `设计规范/03-组件清单.md` C5、`设计规范/02-布局与交互.md` C10 与 `设计规范/原型截图/` 审查 port / adapt / drop；核对既有单工具 C5/C10、diff、permission、工具组间距和两主题快照零 churn，并列出精确新增快照文件。
- [x] 7.3 **用户专属人工视觉停点**：由用户审阅 §7.1 的全部 `.snap.new`；主 agent / 自动实施 agent不得代勾、不得自动 accept。
- [x] 7.4 用户明确批准后，仅 accept §7.1 列出的新快照；不得顺手接受任何其他 `.snap.new`。
- [x] 7.5 重跑 TUI reducer / render / snapshot tests，确认无 `.snap.new` 残留，既有快照逐字节零 churn。

## 8. 文档、兼容与范围锁定

- [x] 8.1 更新 `技术方案/mysteries-agent技术方案.md` 路线图 1.5：保留并行工具节点，但把“按 ReadOnly 推断”改为独立 `ToolConcurrency` + 默认 Exclusive + work-conserving 有界安全段；明确该能力仅是 subagent 的调度前置之一，TUI Interrupt helper 不是通用 Agent / child cancellation。
- [x] 8.2 更新 README 用户能力与 CHANGELOG `[Unreleased]`：多个本地读取 / 搜索可有界并行，变更 / 执行 / Network / 交互仍串行且权限语义不变；不预写 archive 后数量。
- [x] 8.3 核对 `Cargo.toml` / `Cargo.lock` 无新增依赖，config 无并发字段，CLI flags / permission prompt / `PermissionLevel` / Network preview / session JSONL / ToolCard wire 均无变化；`SessionStore::load` raw round-trip 与损坏输入严格报错零回归，normalization 只在两处 activation load site；用专用 12-tool fixture（非 9-tool `default_registry()`）确认完整分类与 delta spec 一致。
- [x] 8.4 记录实现报告中的已知取舍：ready buffer 会延迟公开物理先完成的后项但不阻塞补位；外部文件修改不提供 snapshot；已开始的 `spawn_blocking` 不可硬取消、仍持全局 permit 到结束，若 OS 读取永久挂起会占用容量；TUI-only Interrupt 不覆盖 headless drop。

## 9. 自动化门禁

- [x] 9.1 **targeted**：ToolConcurrency / 12 分类、blocking liveness / process cap / 四文件工具、batch overlap / per-Agent cap / work-conserving refill / 三类 barrier / clamp、ordered occurrence / duplicate id / error isolation、observer、Interrupt occurrence / queue、旧 session history + transcript normalization / 首次 Provider 请求、TUI reducer / render / snapshots 全绿，且无 `.snap.new`。
- [x] 9.2 **format + full Rust**：运行 `cargo fmt --all -- --check`；随后每个独立 PowerShell shell 设置 `$env:CARGO_TARGET_DIR='target/codex-parallel-tools'`，依次运行 `cargo clippy --all-targets --locked -- -D warnings`、`cargo test --locked`、`cargo build --release --locked`。不得 kill 用户进程或回退默认 target 规避锁；隔离 target 仍被锁时报告并停止。
- [x] 9.3 **OpenSpec**：`openspec validate parallelize-safe-tool-calls --strict` 与 `openspec validate --all --strict` 均通过；`openspec status --change parallelize-safe-tool-calls` 为 4/4；`openspec instructions apply --change parallelize-safe-tool-calls --json` 的 remaining 与未勾 checkbox 一致。
- [x] 9.4 **范围**：运行 `git diff --name-only` / `git diff --check` 并审查未跟踪文件、snapshot diff、dependency diff；不得让 `cargo fmt` 制造无关 churn，不得写 `.ai_history`（仅 archive 且用户审阅后写）。

## 10. 真机核验（用户专属；主 agent / 自动实施 agent 均不得代勾）

- [x] 10.1 TUI Normal：让模型在同一回复中同时调用至少两个 `read_file` / `grep` / `glob` / `list_dir`；2–4 项时看到 `并行执行 N 个工具…`。若模型返回 >4 项，看到全部已调度 Running 卡与 `处理 N 个工具（最多并行 4）…`；无需权限框，最终结果 / ToolCard 顺序按 occurrence。
- [x] 10.2 屏障与权限回归：混合本地读取与 `write_file` / `run_shell` / `web_fetch`；读取批次不得跨过变更 / 执行 / Network 调用，Normal / AcceptEdits / Yolo / Plan 的原 permission 行为及单 pending modal 保持现状。
- [x] 10.3 Interrupt：在**无选区、无 modal、无 queue**时让大目录 `grep` / `glob` 并行运行，再按 Esc 或 Ctrl+C；所有 Running 卡收口为 Error + “工具调用已中断”，只出现一次中断 notice，随后立即提交新 Prompt 能正常继续且 Provider 不报 dangling tool result。
- [x] 10.4 `--headless`：要求模型同轮发多个本地读取调用，最终回答正确且进程结束；Network / Edit / Execute 的 y/n prompt 与 reject-only 行为不变。
- [x] 10.5 性能与稳定性：在同一仓库重复执行多读取批次并中断首批后立即发次批，TUI spinner / 输入 / Esc 保持响应；进程级 blocking 上限不造成明显 CPU / 磁盘失控，session 保存后分别经 `--continue` startup 与 `--resume` picker hot-swap 恢复均无 Running 残留或协议错误。

## 11. Archive checklist（不计入 apply progress；不得改为 checkbox）

仅在全部 checkbox 完成、用户完成真机与快照核验并明确发起 archive 后执行：

1. 确认 artifacts 4/4 complete，`openspec instructions apply --change parallelize-safe-tool-calls --json` 为 remaining=0。
2. 展示五份 delta sync 摘要并取得用户选择；sync 后核对主 specs 的 ToolConcurrency、12 工具分类、work-conserving 安全批次 / 三类屏障 / occurrence 顺序、C5/C10、Interrupt 多重集配对与 loaded session 双收口完整落位，并同步更新五个 capability 的 Purpose，避免主 spec 摘要继续写串行 / 未收口的旧语义。
3. 执行 archive move；按实际 archive 目录数量更新 README，不提前写预测值。
4. 按 AGENTS.md 起草本 change 的 `.ai_history/logs/...` archive 决策记录，交用户审阅，并与 archive 进入同一提交。
5. 运行 `openspec validate --all --strict`、`git diff --check`、archive 路径 / 数量与决策记录复核。
