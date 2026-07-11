## Context

当前 `Agent::run_observed` 在 Provider 返回一组 `tool_calls` 后，以单个 `for` 循环依次完成 lookup、纵深检查、permission gate、`execute().await`、history 写入和 observer finished；任一慢读取都会阻塞其后的独立读取。`Tool` 已是 `Send + Sync`，`ToolRegistry` 也能在同一父 future 内稳定借用多个 `&dyn Tool`，但 trait 没有独立的并发安全信息。

不能复用 `PermissionLevel`：它回答“是否需要 Tool permission gate 授权”，而不是“是否能与别的调用重叠”。`submit_plan`、`update_plan`、`ask_user` 都是 `ReadOnly`，却会等待用户或修改进程内计划状态；Network、Edit、Execute 的资源冲突也无法由权限级别表达。已归档的 `add-network-permission-level` 因此明确把 effect / concurrency taxonomy 留给后续 change。

首版可安全 opt-in 的生产工具是 `list_dir`、`read_file`、`glob`、`grep`。它们没有工具级可变状态和外部副作用，但当前主体使用同步 `std::fs`、`ignore::WalkBuilder` 与正则扫描；若只把这些 async trait future 放进并发 stream，同步主体仍会在 poll 时占住 Tokio worker，无法兑现真实延迟收益。

TUI 已按 call id 将 `ToolCallStarted` push 为 C5 Running 卡，并在 `ToolCallFinished` 时回填，因此数据模型能容纳多卡；缺口是 `AgentStatus` / `Phase` 只能表达一个 `ExecutingTool(String)`。当前 Interrupt 由外层 `tokio::select!` drop `run_observed` future，若正处于工具调用，working history 可能留下 `Assistant.tool_calls` 而没有配对 `ToolResult`，Running 卡也不会自动结束。并行会放大这一既有悬空状态，必须在同一 change 收口。

## Goals / Non-Goals

**Goals:**

- 为每个 Tool 提供独立、tool-owned、默认保守的并发策略声明。
- 让同一 Provider 回复中连续、明确安全的本地读取工具以固定上限真实重叠执行。
- 保持 `Exclusive` 屏障、permission gate、Plan 纵深拒、history 顺序和 Provider 协议确定性。
- 单项工具错误局部化，同批其他安全调用继续完成。
- TUI 正确呈现多个 Running 工具和聚合批次状态；Interrupt 后无 Running 卡、无 dangling tool call、无迟到 observer 事件。
- 不新增依赖、配置字段或持久化格式，为后续 subagent 提供有界调度、顺序发布和错误隔离先例；TUI Interrupt 收口不冒充通用 child cancellation。

**Non-Goals:**

- 并行 `web_fetch` / `web_search`，或同时展示多个 Network permission prompt。
- 并行 Edit / Execute、基于 path / command 的资源 key、依赖图或冲突检测。
- 跨 `Exclusive` 调用重排、work stealing、优先级队列或动态并发自适应。
- 暴露用户可配置的并发上限，或按 Provider / model 覆盖上限。
- 并行 Provider 请求、MCP、subagent 本身、通用 effect system，以及供 headless / subagent 调用的通用 Agent cancellation API。
- 强行终止已经进入 Tokio blocking pool 的 OS 级同步读取；Interrupt 只保证结果和事件被丢弃，读取工作无副作用地自然结束。

## Decisions

### D1 · `ToolConcurrency` 独立于 `PermissionLevel`，默认 `Exclusive`

新增二态 enum：

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolConcurrency {
    Exclusive,
    ParallelSafe,
}
```

`Tool` 增加 object-safe 纯函数 `fn concurrency(&self) -> ToolConcurrency`，默认返回 `Exclusive`。工具实现是正向真相源；registry / Agent 只能把调用收紧为独占，不得把工具从 `Exclusive` 提升为 `ParallelSafe`。默认值保证现有与未来 Tool（含未来 MCP adapter）在未审查前 fail-safe 串行。

首版运行时只有同时满足以下条件的调用可进入并行段：tool 存在、`concurrency()==ParallelSafe`、`permission_level()==ReadOnly`、`plan_only()==false`。任一条件不满足即按 `Exclusive` 处理。这是对首版“不并发授权或交互”的 host clamp，不改变两个 enum 的正交语义。

替代方案“所有 ReadOnly 自动并行”被弃：会误并发 `ask_user` / plan 工具，且与主 spec 及 Network change 的既有决策冲突。替代方案“Agent 按工具名维护 allowlist”被弃：分类与实现分离，alias / custom / MCP 工具易漂移；集中策略未来只能用于收紧。

### D2 · 首版只 opt-in 四个本地读取工具

`list_dir`、`read_file`、`glob`、`grep` override 为 `ParallelSafe`。`web_fetch`、`web_search`、`write_file`、`edit_file`、`run_shell`、`submit_plan`、`update_plan`、`ask_user` 均为 `Exclusive`；对后八者用完整分类测试锁定，即使它们依赖默认实现也不能在未来被无意改为并行。

Network 工具逻辑上可能并发，但当前 `ChannelDecider` / `AppState` 只有一个 permission modal，且刚建立 call-scoped preview / SSRF 契约。首版保持它们独占，避免把调度 change 变成并发授权 change。

### D3 · 最大连续安全段 + `Exclusive` 屏障，固定上限 4

Agent 以模型给出的顺序扫描 `tool_calls`：

1. 未注册工具、非并行 eligible 工具各自走既有串行路径，并形成屏障。
2. 相邻的 eligible 调用组成最大连续安全段；前一段必须全部收口后才能执行屏障，屏障完成后才允许启动后一段。
3. 段长度为 1 时沿用 `ExecutingTool(name)`；长度大于 1 时发 `ExecutingTools(count)`，其中 count 固定表示整段已调度调用总数，不表示某一瞬间 active 数。
4. 段内按 provider 顺序发全部 `on_tool_call_started`（表示已纳入批次，可能执行中或等待空槽），再以具名常量 `MAX_PARALLEL_TOOL_CALLS = 4` 执行；窗口补位不重复发送 status。

示例 `[read A, grep B, edit C, glob D]`：A/B 同批，全部结束后 C 独占，C 结束后 D 执行。不得把 D 提前到 C 之前，否则可能读到编辑前的状态。

并发实现复用现有 `futures-util::StreamExt::buffer_unordered(4)`，每个 future 返回 `(original_index, outcome)`。任一物理完成项被 stream 取走后立即空出窗口并补入下一调用，即使 index 0 很慢、index 1 已完成，index 4 也能在 index 0 释放前启动；这是 work-conserving 上限。`join_all` 被弃，因为没有上限；`buffered` 被弃，因为有序队首会阻止已完成后项腾槽、形成 head-of-line 补位阻塞；`tokio::spawn` / `JoinSet` 被弃，因为会迫使 registry 从 `Box<dyn Tool>` 迁到 `Arc<dyn Tool>` 并要求 `'static`，而当前父 future 内的稳定借用已足够。

### D4 · 顺序发布是外部契约，执行重叠是内部行为

安全段维护 `Vec<Option<ToolOutcome>>`（或等价 index-keyed ready buffer）与 `next_publish`。`buffer_unordered` 每产出一个 `(index, outcome)` 就存入对应槽，然后只要 `ready[next_publish]` 存在便连续取出前缀，并按该顺序：

1. push `Message::ToolResult{call_id, content, is_error}`；
2. 调 `observer.on_tool_call_finished(call_id, outcome)`。

因此第二个工具即使先完成，也不会在第一个之前进入 history 或 TUI done 状态，但它已从 in-flight window 移出、允许后续调用补位；这同时保持吞吐与 Provider tool-result 配对的可复现性。整批每个 tool-call occurrence 的结果全部入 history 后才可发下一轮 `provider.complete`。某项 `is_error=true` 仍是普通 outcome，不短路 stream、不取消兄弟调用。

替代方案“finished 按真实完成顺序、history 最后排序”能更早刷新单卡，但会引入 observer 与事实源顺序分裂，并使 Interrupt 时 UI done 与 history missing 不一致，首版不采用。

### D5 · 文件工具用 `spawn_blocking` 兑现真实并行

四个文件工具各自把现有同步主体抽为接收 owned `args` / `ToolContext` 的 helper，并经可测试的 `BlockingToolLimiter` 由 `tokio::task::spawn_blocking` 执行；async `execute` await JoinHandle 并返回原 `ToolOutcome`。join failure（包括 blocking worker panic）转为 `is_error=true` 的 `ToolOutcome`，不让 Agent Loop panic。生产 helper 使用进程共享的 `Arc<Semaphore>`（permit=4），测试使用独立 limiter，避免全局测试互相干扰。

调用先异步取得 owned semaphore permit，再把 permit 一并 move 进 blocking closure；permit 只在真实同步工作结束时释放。若 future 在等待 permit 时被 drop，不启动 closure；若 JoinHandle 被 drop，已启动 closure 仍持 permit。因此连续 Interrupt 后旧 turn 与新 turn 的实际文件 blocking 工作合计仍 ≤4，不会因 detached closure 堆叠突破上限。外层 `buffer_unordered(4)` 另限制单个 Agent 批次的 async execute 数。输出、截断、gitignore、排序、错误文案与 permission level 必须保持不变。

替代方案“改用 `tokio::fs`”只覆盖单文件读取，无法覆盖 `WalkBuilder` / regex 遍历；为四工具统一使用 blocking helper 更一致。

### D6 · observer 与 TUI 增加聚合并行状态，不改变 ToolCard 格式

`AgentStatus` 与 TUI `Phase` 新增 `ExecutingTools(usize)`，只用于长度大于 1 的批次，usize 是整段总数。C10 在 2≤N≤4 时显示 `并行执行 N 个工具…`；N>4 时显示 `处理 N 个工具（最多并行 4）…`，避免把 batch size 误读为 active 数。两者沿用 `accent.primary`、现有 spinner 与 Esc 提示；单调用继续显示 `执行 {name}…`。

C5 不增字段：段内 started 按模型顺序进入同一 transcript，Running 表示已调度（可能 active 或等待窗口），因 finished 延后且有序，批次执行期间自然存在多张 Running 卡。finished 回填时不能假设 call id 全局唯一：AppState 查找 transcript 中**最早的同 id 且仍为 Running**的卡，保证同批重复 id 及跨轮复用 id 都按 occurrence 收口，而不是反复改写第一张 Done 卡。视觉处理：port C5 既有 running/done/error 结构；adapt C10 单名称为批次数量 / 上限提示；drop 新面板、并行动画和 per-tool 独立 spinner。新增并行态、Interrupt 与旧 session recovery 的 Midnight / Daylight `TestBackend + insta` 带色快照，既有 phase matrix 维持原主题覆盖与零 churn。

旧版本可能在 `Interrupted` terminal event 保存仍为 Running 的 ToolCard。为避免加载后永久悬空，或新 turn 复用同一 id 时 finished 先命中旧卡，所有持久化 transcript 在激活前统一经过纯 normalization：每张 Running 卡改为 Error，output=`上次会话已中断`，`truncated=false`、`exit=None`；Done / Error 与其他 block 原样保留。该入口接入代码中仅有的两处 load site：`--continue` 的 startup load，以及 `--resume` picker 选中后的 runtime hot-swap；必须在任何新 turn started 进入 transcript 前完成。normalization 只修改内存中的既有 `ToolCard` 字段，不改变 `SessionStore::load` round-trip 或 session JSONL schema；后续正常 snapshot 会自然写回收口状态。

### D7 · TUI Interrupt 补齐当前 turn 的未配对 `ToolResult`

`run_agent_task` 在把当前 User message 放入 `working` 后记录 `turn_history_start`。若 interrupt arm 获胜：

1. drop `run_observed`，阻止后续公开 outcome / status；
2. 只扫描 `working[turn_history_start..]` 中本 turn 新增的消息，并把每个 `Assistant.tool_calls` 项记录为独立 occurrence（原顺序 + call id）；
3. 遇到 `ToolResult.call_id` 时消费该 id 最早的未配对 occurrence；扫描结束后按原 occurrence 顺序为仍未配对项追加 `ToolResult{is_error:true, content:"tool call interrupted before completion"}`。不得用 `HashSet<call_id>` 判完成，因为同一 turn 的后续 Assistant 可能复用 id；
4. 保存补齐后的 working history，再只发送既有 `AgentEvent::Interrupted`。

旧 session 的 agent history 也可能已持久化 dangling call，不能只修复 transcript。激活 loaded session 前，复用同一 occurrence matcher 逐个处理 `Assistant.tool_calls` 组：该 Assistant 后、下一个非 `ToolResult` 消息前的连续结果按 id 消费最早未配对 occurrence；已有配对结果保持原内容与顺序；仍未配对的 occurrence 按模型顺序在该结果组末尾、下一个非结果消息之前插入同样的 is_error interrupted `ToolResult`。不得以 id 去重，也不得把合成结果统一追加到整段 history 末尾而跨过后续 User / Assistant。该 normalization 与 transcript normalization 一起接入上述两处 load site，必须在任何新 User 输入交给 Agent 前完成；raw `SessionStore::load` 及 load 时磁盘内容保持不变。

UI 在 apply `Interrupted` 时把 transcript 中仍为 Running 的 ToolCard 改为 Error，output 设为“工具调用已中断”，随后保留既有“⊘ 已中断本轮”notice。已调度但尚未进入 active window 的调用已有 Running 卡并一并收口；纵深拒 / unknown 等未发 started 的后续调用可能没有卡片，但在 history 中仍有 interrupted result，使下一轮 Provider 不看到 dangling occurrence。不得为合成结果另发 `ToolCallFinished` 或 `StatusChanged(Idle)`，从而保持既有“Interrupted 后无 trailing event”契约和排队推进逻辑。

替代方案“给 Agent API 新增 CancellationToken 并等待协作式收口”会改动所有 headless 调用方，且无法硬取消 `spawn_blocking`；本 change 采用 TUI 外层 turn checkpoint 的最小闭环。headless 调用方直接 drop `run_observed` 仍没有自动补齐保证，后续 subagent 必须另行设计可复用 cancellation API。

### D8 · 测试以同步原语证明并发，不用 sleep 猜时序

headless 调度测试统一用 per-call oneshot `entered` / `release` / `completed`、测试私有 `Semaphore` permit 与原子 active/max-active 计数：每个通知只发送一次、receiver 在执行前建立，避免 `Notify` 丢信号；失败清理必须 release 或 abort 驱动 task。以两个 entered ack 证明真实重叠，以 5 个调用锁 max-active=4，并在 index 0 保持未释放、index 1 完成时断言 index 4 已 entered，证明 work-conserving 补位。`[safe,safe,exclusive,safe]`、`[safe,unknown,safe]` 与 `ParallelSafe+ReadOnly+plan_only` 分别锁屏障，单项 error 锁错误隔离。

blocking liveness 测试使用 current-thread runtime：closure 发 entered 后等待 std release，独立 async probe 发 ack，外部 OS watchdog 即使断言路径失败也负责 release，最终断言 probe 先于 release；不能用 Tokio timeout 解救被同步阻塞的同一 worker。其他 timeout 只在 `start_paused` 虚拟时钟中作 watchdog，不用 sleep 判并发。

中断集成测试在两个安全工具均 started 后发送 `Interrupt`，断言只收到 `Interrupted`、Running 卡收口、working history 每个 call occurrence 恰有一个 ToolResult；同 turn 重复 id 必须按顺序多重集配对。`run_agent_task` cancellation 与 AppState queue gate 分开测试，后者复用既有 terminal-event 推进 seam。另以真实旧 session fixture 分别锁定 `--continue` startup 与 picker hot-swap 两处 wiring：一组重复 id occurrence 只有首项结果、transcript 留有同 id Running 卡；恢复后 history 先补第二 occurrence、历史卡先变 Error，新 turn 再复用同 id 时首次 Provider 实收 history 无 dangling，finished 只能收口新卡。纯 helper 另覆盖多张 Running、Done / Error / 其他 block 原样保留以及 `truncated` / `exit` 重置。TUI 仅做事后 reducer / `TestBackend + insta` 验证，不把 ratatui 外壳纳入 RED-GREEN。

## Risks / Trade-offs

- **[错误 opt-in 会并发有副作用的工具]** → 默认 `Exclusive`，首版再以 `ReadOnly && !plan_only` clamp，并用 12 工具分类测试锁定；后续新增 opt-in 必须单独审查。
- **[同步文件遍历占满 Tokio worker]** → 统一迁入 `spawn_blocking`；测试 current-thread async probe liveness。
- **[Interrupt 后 blocking closure 仍在后台运行并与新 turn 叠加]** → 仅无副作用读取可进入该路径；进程共享 `Semaphore(4)` permit 移入 closure，真实工作结束才释放，确保跨 turn 总量仍≤4；丢弃迟到 outcome。
- **[较早慢项延迟公开已完成后项]** → `buffer_unordered` 继续 work-conserving 补位，ready buffer 只延迟 history / UI 发布以换取单一事实顺序；各 outcome 已受 `max_output_bytes` 限制。
- **[并行读取与仓库外部修改竞态]** → 与连续串行读取一样不提供文件系统 snapshot；Agent 自身的 Edit / Execute 是屏障，不跨越重排。
- **[Provider 复用 call id 导致错配]** → history 以 occurrence/FIFO 多重集配对；TUI finished 只更新最早同 id Running 卡，并覆盖同批 / 跨轮重复 id。
- **[升级前 session 留有 Running 卡与 dangling history]** → 激活前把 transcript Running 规范化为 Error，并按 Assistant 结果组 / occurrence 补齐 loaded history；旧 fixture 覆盖重复 id 与恢复后首次 Provider 请求，session wire 不变。
- **[补齐 interrupted result 改变既有中断 history]** → 只处理当前 turn suffix 和未配对 occurrence，已完成结果不改、格式不变；专门覆盖 provider 再调用与 session 保存回归。
- **[快照 churn 扩散]** → 新增并行态专用快照；既有单工具 `ExecutingTool`、C5、C10 快照必须零 churn。

## Migration Plan

1. 先落只为编译的 `ToolConcurrency` 接口骨架，再以 TDD 加默认值与 12 工具分类 RED，确认后让四工具 opt-in。
2. 为可测试 blocking limiter 落错误语义 scaffold；以 current-thread liveness / 全局上限 / join failure RED 驱动 `spawn_blocking + Semaphore(4)`，再迁移四工具并锁定输出零回归。
3. 以受控 mock Tool 写出批次重叠、上限、work-conserving 补位、三类屏障、排序和错误隔离 RED；实现 `buffer_unordered + index ready buffer` 后保持全量 headless 测试绿。
4. 接入并行 status、TUI reducer 与 Interrupt history 补齐；事后新增并审阅 Midnight / Daylight 快照。
5. 更新技术方案 / README / CHANGELOG，运行完整验证后进入人工真机测试。

无数据或配置迁移。若需回滚，可把四工具恢复默认 `Exclusive` 并恢复串行循环；session / config 文件无需转换。

## Open Questions

- 无；首版工具集合、per-Agent / 进程 blocking 上限、work-conserving 调度、公开结果顺序、batch-count 文案与 TUI Interrupt 收口均已定案。Network、资源级并发与通用 Agent cancellation 留给独立后续 change。
