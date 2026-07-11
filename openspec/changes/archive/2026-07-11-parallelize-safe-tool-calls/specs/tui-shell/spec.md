## MODIFIED Requirements

### Requirement: 全 phase 状态行 C10

**活动状态行**(输入框上方,见「活动状态行(输入框上方)」requirement)SHALL 据 `StatusChanged` 显示完整 phase(`设计规范/02` 状态机):`Idle`→`◇ 就绪`(idle 简显)、`CallingModel`→`调用模型…`、`ExecutingTool(name)`→`执行 {name}…`、`ExecutingTools(count)` 在 `2≤count≤4` 时→`并行执行 {count} 个工具…`、在 `count>4` 时→`处理 {count} 个工具（最多并行 4）…`、`WaitingForPermission`→`▲ 等待授权…`。`ExecutingTools` 的 count 表示整段已调度 occurrence 总数，不是瞬时 active 数；单工具 MUST 保持 `ExecutingTool(name)`。phase label MUST 渲染在**活动状态行**(输入框上方),MUST NOT 渲染在底部状态行。`AppState` 的 phase 状态 MUST 可单测,渲染 MUST 可 `insta` 快照。

#### Scenario: 单工具 phase 随事件更新(状态可测)

- **WHEN** `AppState.apply(StatusChanged(ExecutingTool("write_file")))`
- **THEN** 其 phase 为 `ExecutingTool("write_file")`,后续渲染**活动状态行**显示 `执行 write_file…`(底部状态行不含 phase)

#### Scenario: 并行 phase 随事件更新(状态可测)

- **WHEN** `AppState.apply(StatusChanged(ExecutingTools(2)))`
- **THEN** 其 phase 为 `ExecutingTools(2)`,后续渲染**活动状态行**显示 `并行执行 2 个工具…`并保留 running spinner / Esc 提示

#### Scenario: 超过窗口时显示总数与真实上限

- **WHEN** `AppState.apply(StatusChanged(ExecutingTools(5)))`
- **THEN** 其 phase 保留总数 5，活动状态行显示 `处理 5 个工具（最多并行 4）…`；窗口补位不反复改写 phase

#### Scenario: 各 phase 活动状态行快照

- **WHEN** 以既有 `Idle` / `CallingModel` / `ExecutingTool(x)` / `WaitingForPermission` matrix 及新增 `ExecutingTools(2)` / `ExecutingTools(5)` 渲染
- **THEN** 既有四态按原主题覆盖与锁定快照零 churn；新增两个并行态分别有 Midnight 与 Daylight `insta` 快照，活动状态行显示正确 label，底部状态行均不含 phase

## ADDED Requirements

### Requirement: 并行工具批次的 C5 / C10 呈现

`ChannelObserver` SHALL 把并行批次的多个 `on_tool_call_started`、聚合 `StatusChanged(ExecutingTools(count))` 与按模型 occurrence 顺序发布的 `on_tool_call_finished` 经既有同一 `mpsc` channel 上送。整段所有 started 在执行窗口启动前按模型顺序上送；C5 Running 表示 occurrence 已调度、可能 active 或等待最多 4 个空槽。`AppState` SHALL 按 started 到达顺序在单一 transcript 中建立多个 C5 Running `ToolCard`；finished 必须回填 transcript 中最早的“同 id 且仍为 Running”的卡，不得假设 id 全局唯一或反复改写第一张 Done 卡。不得新增独立并行面板或改变 `ToolCard` 持久化字段。

任何从 session JSONL 加载的持久化 transcript MUST 在激活及接收新 turn started 前，把所有遗留 Running 卡规范化为 Error、output=`上次会话已中断`、`truncated=false`、`exit=None`；Done / Error 卡和其他 block MUST 保持不变。该规则 MUST 接入两处 load site：`--continue` 启动加载，以及 `--resume` picker 选中后的 runtime hot-swap；不得改变 raw `SessionStore::load` round-trip、要求 session wire migration 或在 load 时改写磁盘。

视觉 SHALL port `设计规范/03-组件清单.md` C5 的 running / done / error 结构，adapt `设计规范/02-布局与交互.md` C10 单工具名称为并行数量，并 drop 新面板、并行动画与 per-tool 独立 spinner。连续 Running 卡沿用既有工具组紧凑布局和全局 `Ctrl+O` 规则；既有单工具、diff、permission 与非并行工具卡快照 MUST 零 churn。

#### Scenario: 两个 started 形成两张同时 Running 的工具卡

- **WHEN** channel 按 `[ToolCallStarted(call-1), ToolCallStarted(call-2), StatusChanged(ExecutingTools(2))]` 到达且尚无 finished
- **THEN** transcript 按 call-1 → call-2 含两张 Running C5 卡，活动状态行显示 `并行执行 2 个工具…`；Midnight 与 Daylight `TestBackend + insta` 带色快照与锁定基线一致

#### Scenario: 有序 finished 按 id 收口对应卡片

- **WHEN** 上述两张 Running 卡再依次收到 `ToolCallFinished(call-1)`、`ToolCallFinished(call-2)`
- **THEN** 每个 outcome 只更新同 id 卡片，最终两卡均非 Running，transcript 顺序不变、不 panic

#### Scenario: 五项批次区分已调度与 active 上限

- **WHEN** 五个 started 后收到 `ExecutingTools(5)`，但 Agent active 计数锁定为 4
- **THEN** transcript 可有五张 Running（均已调度，含一张等待窗口），活动状态行显示 `处理 5 个工具（最多并行 4）…`；不得显示“并行执行 5 个工具”或新增 queued 持久化状态

#### Scenario: 重复 id 只回填最早 Running occurrence

- **WHEN** transcript 中已有一张 `id=call-1` 的 Done 卡，随后新增两张同 id 的 Running 卡并按 occurrence 顺序收到两次 finished
- **THEN** 第一次 finished 更新较早 Running 卡、第二次更新较晚 Running 卡，原 Done 卡不变；最终不存在同 id Running 残留

#### Scenario: 旧 session 的 Running 卡在新 turn 前收口

- **WHEN** `--continue` 启动或 `--resume` picker 选中后的 hot-swap 加载的 transcript 含一张 Running `id=call-1`，随后新 turn 再 started / finished 同 id 的调用
- **THEN** 历史卡在新 turn 前已变 Error 且 output 为“上次会话已中断”，finished 只更新新卡；最终无 Running 残留、历史卡不被改写，Midnight 与 Daylight `TestBackend + insta` 带色快照与锁定基线一致

#### Scenario: 单工具 C5 与 C10 零回归

- **WHEN** 仍只收到一个 `ToolCallStarted`、`StatusChanged(ExecutingTool(name))` 与一个 finished
- **THEN** C5 卡、活动状态行、工具组间距与既有单工具锁定快照逐字节一致，不出现“并行执行 1 个工具”

### Requirement: 并行批次 Interrupt 的卡片与 history 收口

`run_agent_task` 在当前 User message 已加入 working history 后 SHALL 记录本 turn 的 history 起点。若独立 interrupt arm 获胜，它 MUST drop 当前 `run_observed` future，只扫描本 turn 新增 suffix：每个 `Assistant.tool_calls` 项按出现顺序登记为独立 occurrence；每个 `ToolResult.call_id` 消费相同 id 最早的未配对 occurrence；扫描结束后按原 occurrence 顺序为每个仍未配对项追加且仅追加一个 `ToolResult{is_error:true, content:"tool call interrupted before completion"}`。实现 MUST 使用 occurrence / FIFO 多重集语义，不得以 `HashSet<call_id>` 假设 id 唯一；已配对结果 MUST 保留且不得重复。补齐后保存 working history，再只发送既有 `AgentEvent::Interrupted`，不得发送合成 `ToolCallFinished` 或 trailing `StatusChanged(Idle)`。

`AppState.apply(Interrupted)` SHALL 把 transcript 中仍为 Running 的 C5 卡全部改为 Error，output 设为“工具调用已中断”，再保留既有「⊘ 已中断本轮」notice 与 queue 推进语义。已经进入 blocking pool 的只读 closure 允许自然结束并继续持有进程共享 permit，但其 outcome MUST 被丢弃、不得产生迟到 finished / history 写入；新 turn 的读取须等待 permit，跨 turn 实际 blocking 工作仍≤4。agent task 与程序保持存活。

任何 loaded session 的 agent history MUST 在激活及接收新 User 输入前完成 occurrence-aware normalization。对每条含 tool calls 的 Assistant，以其后、下一个非 `ToolResult` 消息前的连续结果为一组：每个已有结果消费同 id 最早未配对 occurrence；已有配对结果 MUST 保持原内容与顺序；每个剩余 occurrence MUST 按模型顺序在该组末尾、下一个非结果消息前插入且仅插入一个 `ToolResult{is_error:true, content:"tool call interrupted before completion"}`。不得按 id 去重，不得把合成结果跨过后续 User / Assistant 统一追加到 history 末尾。该规则 MUST 与 loaded transcript normalization 一起接入 `--continue` startup 与 `--resume` picker hot-swap 两处 load site，且 raw load / 磁盘内容 MUST 保持不变。

#### Scenario: 中断两项并行批次后 history 无 dangling call

- **WHEN** 两个安全工具均发出 entered ack 且仍等待各自 release oneshot 时投入 `Interrupt`
- **THEN** working history 中两个 occurrence 各有且仅有一个 is_error interrupted ToolResult；只收到 `AgentEvent::Interrupted`，虚拟时钟 watchdog 内无 finished / Idle，agent task 继续存活；失败清理须 release / abort 驱动任务

#### Scenario: 已完成前缀不重复、未完成后缀补齐

- **WHEN** 三个调用中 call-1 已按序完成并写入正常 ToolResult，call-2 / call-3 尚未公开结果时中断
- **THEN** call-1 保持原正常结果且不重复，call-2 / call-3 按原顺序各补一个 interrupted ToolResult，下一轮 Provider 不接收 dangling tool call

#### Scenario: 同一 turn 复用 call id 仍补后一 occurrence

- **WHEN** 本 turn 较早 Assistant 的 `call-1` 已有正常 ToolResult，较后 Assistant 再用 `call-1` 且在结果前被中断
- **THEN** 较早 occurrence 保持一个正常结果，较后 occurrence 追加一个 interrupted 结果；不得因 id 已出现就漏补

#### Scenario: 旧 session 的 history 与 transcript 在首次 Provider 请求前双收口

- **WHEN** 旧 session 的一个 Assistant 组含两个同 id `call-1` occurrence、其后只有第一个正常 ToolResult，transcript 另有一张同 id Running 卡；经 `--continue` startup 或 `--resume` picker hot-swap 加载后提交新 Prompt
- **THEN** 第一个结果保持不变，第二 occurrence 在该组末尾、后续非结果消息之前得到一个 is_error interrupted ToolResult；历史 Running 卡变 Error / “上次会话已中断”；首次 Provider 实收 history 中两个 occurrence 均各有一个结果且无 dangling call，不因重复 id 漏补或改写既有结果

#### Scenario: Interrupted 收口全部 Running 卡(双主题快照)

- **WHEN** transcript 含两张 Running C5 卡并 apply `AgentEvent::Interrupted`
- **THEN** 两卡均变 Error、output 为“工具调用已中断”，末尾仍有非致命「⊘ 已中断本轮」notice；Midnight 与 Daylight `TestBackend + insta` 带色快照与锁定基线一致

#### Scenario: 中断并行批次后排队 Prompt 继续推进

- **WHEN** 并行批次按 run-agent cancellation seam 收口后，再以既有 `handle_agent_event(Interrupted)` queue gate 驱动一条待处理 Prompt
- **THEN** cancellation seam 不消费 `input_rx` 中的后续 Prompt，UI terminal-event gate 只 dequeue 一条并启动下一轮；两层测试分别断言，后台只读 closure 的自然结束不得污染新一轮事件或 history
