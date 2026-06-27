# 2026-06-27 · 10 · archive add-tui-events

## 决策

- **TUI cut2a:结构化观测事件 + 工具卡 + 全 phase**,让工具调用 / phase 在 TUI 可视 | 主导:cut1 提案定的 cut2 拆分前半 | 依据:§3 / §8 / 设计规范 C5/C10
- **D1 事件接口 = `AgentObserver` trait + 新方法 `run_observed`**(非扩 `DeltaSink`、非改 `run` 签名)| 弃:扩 `DeltaSink`(污染文本 sink 语义 + 连带改 provider-abstraction,跨两 capability)、换 event sink / 改 `run` 签名(破所有 caller + 9 测试)| blast 最小:只 agent-loop 一条 additive ADDED requirement
- **D2 `run` 委托 `run_observed(.., &NoopObserver)` 逐字节等价**;**既有 9 个 agent-loop 测试零回归作硬闸**(改 archived capability 的安全闸)| 主 agent 已逐路核对 history push 三路(unknown/denied/executed)不变
- **D3 发射点 + 边界**:CallingModel / ToolCallStarted{readonly=permission_level==ReadOnly} / WaitingForPermission(非只读)/ ExecutingTool / ToolCallFinished / Idle;拒绝·未知合成 is_error `ToolOutcome` 统一上报;顺序由记录型 observer TDD 钉死
- **D4/D5 `AgentStatus`/`AgentObserver` 置 agent core**;tui 经 `ChannelObserver`(mirror `ChannelSink`)forward + 扩 `AgentEvent` 三变体(`ToolCallFinished` 携 `ToolOutcome`,因其 `Clone`;`AgentEvent` 仍不 `Clone`)
- **D6 渲染结构态 / 最小色 / 静态字符**(glyph `✓`/`✗`/`◇`/`▲`);全 `theme.rs` 主题、spinner 动画留 cut2b
- **D7 测试分界**:事件发射(`run_observed` 序列 + `run` 零回归)/ `ChannelObserver` / `run_agent_task` / `AppState` = red-green(Mock + 记录型 observer);render = insta;新 C5 首帧 = 结构对眼
- **capability delta**:agent-loop ADDED(observer 需求,既有 3 条 requirement 不变)+ tui-shell ADDED(事件 / C5 / C10);均 additive
- **两停点**:§1.1 observer 接口红灯(首次改 archived agent-loop,主 agent 审接口 + 发射序列通过)+ §6.2 C5 首帧结构对眼
- **审查修正**:① §6.2 对眼**打回**——工具卡渲染了 `ToolOutcome` 没有的数据(`exit {code}` foot 从 is_error 造、`+N` 截断行数)→ 删 exit foot、截断标记去假 N(`⋯ 输出已截断(超出 max_output_bytes)`)、error mock 改 `command failed: permission denied`、design 记 C5 偏离 | **依据:权威次序 code(`ToolOutcome{content,is_error,truncated}`)> 设计规范 C5,显式标注冲突** | 主 agent 对眼;② clippy 2 处 single-pattern `match` → `if let`(主 agent 审查,clippy 维持零警告)
- **cut1 两帧状态行补 `◇`/`▲` glyph**(对原型 midnight-01/02 的合法提升)→ 重锁
- **里程碑**:工具调用 / phase 在 TUI 可视;agent-loop 首次安全 MODIFY(零回归)

## 变更

- MODIFY `src/agent/mod.rs`(`AgentObserver`/`AgentStatus`/`NoopObserver`/`run_observed`,`run` 委托)+ `src/tui/{channel,app,mod,render}.rs`(扩事件 / `ChannelObserver` / 工具卡 / 全 phase)
- 验证:`cargo test` 119 passed / 1 ignored(既有 9 agent-loop 零回归 + 6 insta 快照);`clippy` 零警告;`fmt` 通过;**零新依赖**(`Cargo` 无 diff)
- archive:`changes/add-tui-events` → `changes/archive/2026-06-27-add-tui-events`;`specs/` agent-loop +1、tui-shell +3 requirements(均 ADDED)

## 待决

- **cut2b `add-tui-theme`**:全 `01-设计令牌` 主题(`theme.rs` + token 单测,Midnight/Daylight)+ C6 diff body + C7 致命框 + transcript 滚动 + spinner 动画 + insta 全锁 + 6 themed 帧对眼
- **exit foot / 截断行数**:需给 `ToolOutcome` 加 exit/count 字段(数据模型变更)→ 留 tool-system / 收尾 change;cut2a 已 defer 并在 design 标注
- 内置命令(C8/C9,`/help` 等)、Anthropic、`tool_mode` 降级、step5 收尾(流式 / 超时 / 重试)
- 极小边界:unknown-tool 仅发 `ToolCallFinished`(无 `ToolCallStarted`),UI 处理良性;确认-放行序列未在 agent-loop 单元层单测(§4 run_agent_task 间接覆盖)

## 引用

- change:`add-tui-events`(rationale / rejected alternatives 全量见 design.md D1–D7;archive 路径 `changes/archive/2026-06-27-add-tui-events`)
- 技术方案 §3 / §8 / §9 / §12 step4
- `设计规范/03-组件清单`(C5/C10)、`02-布局与交互`、`原型截图/midnight-02-权限态.png`(首帧结构对眼)
- 前置 change:`add-tui-shell`(决策记录 09)
- session log:无专属 checkpoint —— 子 agent propose + implement(两停点:§1.1 observer 接口、§6.2 C5 对眼);主 agent review(核 agent-loop MODIFIED delta 最小 + 零回归、§6.2 对眼打回工具卡造数据、抓 clippy)+ commit / archive
