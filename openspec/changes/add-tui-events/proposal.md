## Why

`add-tui-shell`(cut1)立起了 §3 双-task + channel 骨架,但工具执行「不可见」——Loop 只经 `DeltaSink`(文本)与 `PermissionDecider`(权限)露面,没有结构化事件,故 transcript 渲不出工具卡、状态行只有粗 phase。本 change(**TUI cut2 的 a 半**)给 Agent Loop 加结构化观测事件,让工具调用与 phase 可视(`设计规范/` C5 工具卡 + C10 全 phase)。这是 TUI 工作**首次改 archived 的 `agent-loop`**,故刻意把改动面压到最小、既有行为零回归。全主题 / C6 diff / C7 / 滚动留 cut2b(`add-tui-theme`)。

## What Changes

- **MODIFY `agent-loop`(additive,零回归)**:
  - 新增 `AgentObserver` trait(`on_status` / `on_tool_call_started` / `on_tool_call_finished`,**全 default no-op**)+ `AgentStatus`(`Idle` / `CallingModel` / `ExecutingTool(String)` / `WaitingForPermission`)+ `NoopObserver`。
  - 新增 `Agent::run_observed(history, ctx, sink, observer)`:在 模型调用前 → `CallingModel`、工具分发 → `ExecutingTool(name)` + `ToolCallStarted{id,name,args,readonly}`、需确认 → `WaitingForPermission`、工具完成 → `ToolCallFinished{id,outcome}`、循环末 → `Idle` 经 observer 发事件。
  - 既有 `Agent::run` **签名不变**,内部委托 `run_observed(.., &NoopObserver)` —— 既有 9 个 agent-loop 测试 + `cli` / `e2e` / `run_single_turn` **零改动、零回归**。`DeltaSink` / provider-abstraction **不动**。
- **MODIFY `tui-shell`(additive)**:
  - 扩 `AgentEvent` += `ToolCallStarted{id,name,args,readonly}` / `ToolCallFinished{id,outcome}` / `StatusChanged(AgentStatus)`。
  - 新增 `ChannelObserver`(impl `AgentObserver`,forward → channel,mirror 既有 `ChannelSink`/`ChannelDecider`);`run_agent_task` 改调 `run_observed(.., &ChannelObserver)`。
  - `AppState` 据新事件维护工具卡块 + 全 phase;`render` 加 C5 工具卡(名 / args / 只读徽章 / 状态 glyph / output / exit / 截断)与 C10 全 phase 状态行。

### 4 点定夺(已与你确认 Option 1)

1. **拆分**:本 = cut2a(事件 + 工具卡 + 全 phase);cut2b `add-tui-theme` = 全主题 `theme.rs`(01 令牌)+ C6 diff body + C7 致命框 + 滚动 + insta 全锁 + 6 帧对眼。
2. **agent-loop 接口** → `AgentObserver` + 新方法 `run_observed`,既有 `run` 委托 `NoopObserver`(见 design D1/D2)。**blast 最小**:只 `agent-loop` 一个 additive ADDED requirement,`DeltaSink`/provider-abstraction 不动,既有测试零回归。tui 经 `ChannelObserver` + 扩 `AgentEvent` 衔接。
3. **主题**(cut2b)→ 本 change 渲染仍**结构态 / 最小色**;全 `01-设计令牌` 主题(`theme.rs` + token 単測)留 cut2b。
4. **对眼**(`设计规范/README` + config.yaml 强制)→ 本 change 对 **新 C5 工具卡 / C10 phase 的首帧做结构对眼**(对 `原型截图/midnight-02-权限态` 的工具卡 / 状态行区域,**只核结构、不核配色**);完整 6 帧 themed 对眼留 cut2b。

**port/adapt/drop(cut2a)**:port ✅ = C5 工具卡结构(名 / args / output / exit / 截断)、glyph `✓`/`✗`/`◇`/`▲`、C10 phase label;adapt ⚠️ = 圆角 → box/缩进、只读徽章 `只读 · 自动运行` 文本、`CallingModel`/`ExecutingTool` 的 spinner → **本 change 用静态 label**(动画留 cut2b);drop ❌ = 阴影 / 动画。**显式偏离**:本 change 不实装 `theme.rs` 主题(最小色),spinner 用静态字符。

## Capabilities

### New Capabilities

<!-- 无。本 change 扩展既有 agent-loop 与 tui-shell 两个能力,不新建 capability。 -->

### Modified Capabilities

- `agent-loop`: ADDED —— 结构化观测事件(`AgentObserver` + `run_observed`);既有「多轮编排 / max_iterations / 错误分流」三条 requirement **不变**,`run` 委托后行为一致。
- `tui-shell`: ADDED —— 扩展 `AgentEvent`(工具/状态事件)+ `ChannelObserver` 上送 + C5 工具卡渲染 + C10 全 phase 状态行;cut1 既有 requirement 不变。

## Impact

- **改动代码**:`src/agent/mod.rs`(+`AgentObserver`/`AgentStatus`/`NoopObserver`/`run_observed`,`run` 委托)、`src/tui/{channel, app, render, mod}.rs`(扩事件 + `ChannelObserver` + 工具卡 / phase)。
- **新增依赖**:**无**(纯逻辑 + 既有 ratatui/tokio)。
- **构建 / 测试**:`agent-loop` 事件发射走 **red-green**(Mock + 记录型 observer 断言事件顺序),**既有 9 个 agent-loop 测试保持绿**(核心行为零回归);`ChannelObserver` / `run_agent_task` / `AppState` 走测;工具卡 / phase 渲染走 `insta`(结构),首帧结构对眼。`cargo test` 默认全绿、无终端 / 不触网。
- **里程碑**:本 change 后 TUI transcript 可视化工具调用(running/done/error 卡)+ 状态行实时 phase;cut2b 上主题后即 §8 完整观感。
- **下游契约**:`AgentObserver` 是 §3 完整事件模型的落地缝;后续(命令 / 体验)可经同缝加更多观测。
