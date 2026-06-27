## 1. AgentObserver + run_observed(强制 TDD · 停点 · 动 agent-loop)

- [x] 1.1 【红 · 停点】写 `run_observed` 事件测试:记录型 observer + Mock 脚本(工具轮 + 终复),断言事件序列 `CallingModel → ToolCallStarted →（ExecutingTool / WaitingForPermission）→ ToolCallFinished → CallingModel → Idle`;含权限拒绝路径(`WaitingForPermission` + is_error `ToolOutcome` 的 `ToolCallFinished`)。确认失败。**贴 `AgentObserver` / `AgentStatus` / `run_observed` 草案 + 失败输出,停下等确认**(新观测接口 + 首次改 archived agent-loop)
- [x] 1.2 【绿】实现 `AgentObserver`(三方法 default no-op）/ `AgentStatus` / `NoopObserver`(agent 模块）+ `run_observed`(emission 点见 design D3;拒绝/未知合成 `ToolOutcome`)
- [x] 1.3 【重构】清理

## 2. run 委托 + 零回归闸

- [x] 2.1 【绿】`Agent::run` 改为 `self.run_observed(history, ctx, sink, &NoopObserver).await`(签名不变,见 design D2)
- [x] 2.2 【验收】既有 9 个 agent-loop 测试(调 `run`)+ `cli` / `e2e` / `run_single_turn` 全部**保持绿**(核心行为零回归);`cargo test` 全绿

## 3. 扩 AgentEvent + ChannelObserver(走测)

- [x] 3.1 【红】写 `ChannelObserver` 测试:`on_status` / `on_tool_call_started` / `on_tool_call_finished` → channel 收 `StatusChanged` / `ToolCallStarted` / `ToolCallFinished`(对应字段正确);确认失败
- [x] 3.2 【绿】`AgentEvent` += `ToolCallStarted{id,name,args,readonly}` / `ToolCallFinished{id,outcome}` / `StatusChanged(AgentStatus)`(仍不 derive `Clone`)+ 实现 `ChannelObserver`(mirror `ChannelSink`,见 design D5)
- [x] 3.3 【重构】清理

## 4. run_agent_task 接 run_observed(走测 · Mock 事件流)

- [x] 4.1 【红】写测试(Mock 工具脚本 + tempdir，对权限回 `Allow`):驱动 `run_agent_task` → channel 依次见 `StatusChanged(CallingModel)` / `ToolCallStarted` / `ToolCallFinished` / 文本 / `TurnComplete`(无终端）;确认失败
- [x] 4.2 【绿】`run_agent_task` 从同一 `ui_tx` 造 `ChannelSink` + `ChannelObserver`,改调 `agent.run_observed(.., &sink, &observer)`(见 design D5)
- [x] 4.3 【重构】清理

## 5. AppState 工具卡 + 全 phase(状态走测)

- [x] 5.1 【红】写 `AppState` 测试:`apply(ToolCallStarted)` 增 running 工具卡块、`apply(ToolCallFinished)` 转 done/error + output/exit;`apply(StatusChanged(ExecutingTool("write_file")))` → phase 更新;确认失败
- [x] 5.2 【绿】`AppState` 加工具卡块列表 + 全 phase 字段 + `apply` 处理三新事件(见 design D6)
- [x] 5.3 【重构】清理

## 6. 渲染 C5 工具卡 + C10 phase(insta · 结构 · 首帧结构对眼停点)

- [x] 6.1 【绿】`render` 加 C5 工具卡(头 glyph+名+args+只读徽章 / 体 output+截断 / 脚 exit,`设计规范/03` C5)+ C10 全 phase 状态行(`02` 状态机;最小色 / 静态字符,见 design D6)
- [x] 6.2 【insta · 停点】写 `TestBackend` 快照:工具卡 running/done/error 三态 + 各 phase 状态行;`cargo insta review` —— **新 C5 工具卡首帧对 `原型截图/midnight-02-权限态` 工具卡 / 状态行区域做结构对眼**(只核结构、不核配色;config.yaml 首个 UI 组件人工审）。**贴首帧渲染给用户审**

## 7. 收尾

- [x] 7.1 `cargo build`、`cargo test` 默认全绿(事件 red-green + 既有零回归 + insta,无终端 / 不触网)、`cargo fmt`
- [x] 7.2 自检:`agent-loop` ADDED(observer 变体)+ `tui-shell` ADDED(事件 / C5 / C10)requirements 全有落点(走测 / insta / 结构对眼 已分类);偏离已标注(结构态非主题、spinner 用静态、cut2b 留项);**既有 9 agent-loop 测试零回归**已验
