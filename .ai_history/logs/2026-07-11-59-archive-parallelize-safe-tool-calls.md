# 2026-07-11 · 59 · archive parallelize-safe-tool-calls

## 决策
- 工具并发能力使用独立 `ToolConcurrency` 元数据，默认 `Exclusive`；首版仅 `list_dir` / `read_file` / `glob` / `grep` opt-in `ParallelSafe` | 选:独立并发分类 | 弃:按 `PermissionLevel::ReadOnly` 推断并行安全（会误包含交互与计划工具） | 主导:讨论收敛 | 依据:spec/code/tests
- Agent Loop 只并行最大连续安全段，固定上限 4，并采用 work-conserving 补位；`Exclusive`、未知工具、非 `ReadOnly` 与 `plan_only` 均形成不可跨越屏障 | 选:`buffer_unordered` + 有序 ready buffer | 弃:`join_all`（无上限）、`buffered`（队首阻塞补位）、跨屏障重排（改变可见状态） | 主导:讨论收敛 | 依据:spec/tests
- 工具物理完成可乱序，但 `ToolResult`、observer finished 与 Provider 可见顺序保持模型 occurrence 顺序；重复 call id 按 occurrence 多重集处理，不作为去重键 | 选:original index 有序发布 | 弃:按物理完成顺序公开（破坏 history 与 UI 确定性） | 主导:讨论收敛 | 依据:tests
- 四个同步文件工具经 `spawn_blocking` 执行，并共享进程级 `Semaphore(4)`；permit 移入 blocking closure，Interrupt 后已启动工作自然结束且继续占用容量 | 选:有界自然收口 | 弃:drop future 即释放 permit（会突破真实进程上限）、强行终止 blocking worker（Tokio 不提供安全硬取消） | 主导:讨论收敛 | 依据:code/tests
- TUI Interrupt 与 loaded session activation 均按 occurrence / FIFO 补齐 dangling `ToolResult`，并把 Running 卡收口为 Error；该能力只覆盖 TUI turn 与 session 恢复，不定义通用 Agent / subagent cancellation API | 选:局部双收口 | 弃:只按 call id 去重、只修 transcript、不完整扩张通用取消协议 | 主导:讨论收敛 | 依据:spec/tests/真机验证
- TUI 沿用 C5 工具卡并允许多张 Running；C10 显示批次总数及“最多并行 4”，不新增并行面板、queued 持久化状态或 per-tool spinner | 选:port C5 + adapt C10 | 弃:新增专用并行面板（增加视觉与状态复杂度） | 主导:用户批准快照 | 依据:设计规范/快照/真机验证
- 代码审查修复后，`--continue` provider fallback 使用启动 provider/model，interactive switch 与 session restore 采用显式类型区分；Continue、picker、Interrupt 与并发上限测试均覆盖真实生产入口并移除 timing 假阳性 | 选:生产 seam + 确定性 ack/mutation 验证 | 弃:测试复制生产分支、固定 sleep 判断并发 | 主导:代码审查 | 依据:mutation/tests

## 变更
- 新增 `ToolConcurrency` 与 12 个内置工具并发分类。
- Agent Loop 接入连续安全段、上限 4、work-conserving 补位、有序结果发布和错误隔离。
- 四个本地读取工具迁入有界 `spawn_blocking` worker。
- TUI 接入 `ExecutingTools`、多 Running ToolCard、Interrupt 收口和 session activation normalization。
- 更新 README、CHANGELOG、技术方案及五个 OpenSpec capability。
- 自动化门禁、八份双主题快照和 10.1–10.5 真机验证全部通过。

## 待决
- 本 change 无遗留项。
- MCP、subagent 实装、通用 Agent cancellation、Network/Edit/Execute 并行及可配置并发数留待后续 change。

## 引用
- OpenSpec change:`parallelize-safe-tool-calls`
- 跨 session 决策记录:`2026-07-10-58-archive-add-network-permission-level.md`
