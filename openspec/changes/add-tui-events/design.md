## Context

`add-tui-shell`(cut1,已 archived)立起 §3 双-task + channel + ratatui 四区外壳,但只经 `DeltaSink`(文本)/ `PermissionDecider`(权限)观测,工具执行不可见、状态行只有粗 phase。本 change = TUI cut2 的 **a 半**:给 Agent Loop 加结构化观测事件,渲工具卡 C5 + 全 phase C10。**首次改 archived 的 `agent-loop`**,故压最小改动面、既有行为零回归(用户已确认 `run_observed` 取法)。全主题 / C6 diff / C7 / 滚动留 cut2b。

现状(real code):`Agent::run(&self, history, ctx, sink: &dyn DeltaSink)`,循环点 `provider.complete(.., sink)` / `registry.get` / `gate(decider)` / `tool.execute`;`tui::AgentEvent { TextDelta, PermissionRequired, TurnComplete, Error }`(非 `Clone`)、`ChannelSink` / `ChannelDecider`;`ToolOutcome` derive `Clone`。视觉权威:`theme.rs`/insta > `设计规范/`(C5/C10/02) > 原型 > 推断。

## Goals / Non-Goals

**Goals:**

- `AgentObserver` + `AgentStatus` + `Agent::run_observed`;`run` 委托 `NoopObserver`,**既有 9 测试零回归**、`DeltaSink` 不动。
- tui:扩 `AgentEvent` + `ChannelObserver` + `AppState`/`render` 工具卡 C5 + 全 phase C10(结构态)。
- 事件发射 red-green(Mock 断序);渲染 insta(结构);新 C5 首帧结构对眼。

**Non-Goals(留 cut2b):**

- 全 `01-设计令牌` 主题(`theme.rs` + token 単測)、themed 渲染、spinner 动画。
- C6 diff body、C7 致命框、transcript 滚动、insta 全锁、6 themed 帧对眼。

## Decisions

- **D1 事件接口 = `AgentObserver` trait + 新方法 `run_observed`(非扩 `DeltaSink`、非改 `run` 签名)。** 用户已确认。`AgentObserver`(`Send + Sync`)三方法**全 default no-op**;`AgentStatus { Idle, CallingModel, ExecutingTool(String), WaitingForPermission }`;`NoopObserver`(空 impl)。**blast 最小**:`DeltaSink` / provider-abstraction 不动(observer 是 Loop 自己的观测面,不把工具/状态塞进文本 sink);只 `agent-loop` 一个 additive ADDED requirement。备选:扩 `DeltaSink` 加 no-op 方法(弃:污染文本 sink 语义、连带改 provider-abstraction,跨两 capability);换 event sink / 改 `run` 签名(弃:破既有所有 caller 与 9 测试,blast 大)。

- **D2 `run` 委托 `run_observed(.., &NoopObserver)`,逐字节等价。** 既有 `Agent::run` 签名与契约不变,实现体下沉到 `run_observed`,`run` 仅 `self.run_observed(history, ctx, sink, &NoopObserver).await`。**验收硬约束**:既有 9 个 agent-loop 测试(调 `run`)全程保持绿(核心行为零回归)——这是本 change 改 archived capability 的安全闸。

- **D3 emission 点与边界。** 模型调用前 `on_status(CallingModel)`;每个 tool_call:`on_status(ExecutingTool(name))` + `on_tool_call_started{id,name,args,readonly}`(`readonly = permission_level()==ReadOnly`),`RequiresConfirmation` 询问前 `on_status(WaitingForPermission)`,产出后 `on_tool_call_finished{id,outcome}`;循环自然终止前 `on_status(Idle)`。**边界**:拒绝 / 未知工具无 `execute`,合成一条 is_error 的 `ToolOutcome`(user denied / unknown tool)喂 `on_tool_call_finished`,使 C5 卡统一显 error;精确顺序由 TDD 钉死(记录型 observer 断言序列)。`MaxIterations` / provider 错误路径:状态可发 `Idle` 或不发,行为以「既有 run 等价」为准。

- **D4 `AgentStatus` / `AgentObserver` 置 agent 模块(core)。** Loop 发它们,属 agent-loop;tui 复用(`ChannelObserver` forward、`AppState` 存 phase)。`on_tool_call_finished` 取 `&ToolOutcome`(`Clone`),tui 侧 `.clone()` 入 `AgentEvent`。

- **D5 tui 衔接 = `ChannelObserver`,mirror cut1。** `ChannelObserver { tx: UnboundedSender<AgentEvent> }` impl `AgentObserver`,三回调 `tx.send(StatusChanged/ToolCallStarted/ToolCallFinished)`(sync send,契合 sync observer 方法)。`AgentEvent` 扩三变体(`ToolCallFinished{outcome: ToolOutcome}` 因 `ToolOutcome: Clone` 可入;`AgentEvent` 仍不 derive `Clone`)。`run_agent_task` 从同一 `ui_tx` 造 `ChannelSink` + `ChannelObserver`,调 `run_observed(.., &sink, &observer)`。

- **D6 渲染结构态,主题留 cut2b。** `AppState` 加工具卡块列表(`{id,name,args,readonly,status,output,truncated}`)+ 全 phase 字段;`apply` 处理三新事件;`render` 加 C5 工具卡 + C10 phase 状态行。**显式偏离**:C5 的 exit foot 与截断行数 N 需要 `ToolOutcome` 带 exit/count 字段,属数据模型变更且超 cut2a 范围;本 cut 只渲 `ToolOutcome { content, is_error, truncated }` 支持的字段,真实 exit foot 留后续 tool-system/收尾 change。**最小色 / 静态字符**:`running` 用静态占位(非 spinner)、glyph `✓`/`✗`/`◇`/`▲`(01 glyph port);全 `theme.rs` token 上色留 cut2b。

- **D7 测试分界。** 事件发射(`run_observed` 序列 + `run` 零回归)、`ChannelObserver`、`run_agent_task` 事件流、`AppState` 工具卡/phase 状态 = **red-green 走测**(Mock + 记录型 observer + 程序化 channel,无终端);工具卡 / phase **渲染** = `TestBackend` + `insta`(结构);新 C5 首帧 = **结构对眼**(对 `原型截图/midnight-02` 工具卡 / 状态行区域,只核结构不核色,满足 config.yaml「首个 UI 组件快照人工审」)。

## Risks / Trade-offs

- **[改 archived `agent-loop`]** → 缓解:D1/D2 把改动压成「新 trait + 新方法 + `run` 委托」,既有 requirement / 9 测试零触动;ADDED(非 MODIFIED 既有文字)delta;委托等价由「既有测试保持绿」硬闸守。
- **[emission 边界(拒绝/未知/超限)易漏]** → 缓解:D3 合成 `ToolOutcome`;记录型 observer 测试覆盖拒绝 / 未知 / 正常三路;以「run 等价」为不变量。
- **[结构态快照与 themed 截图不像]** → 缓解:cut2a 只结构对眼(忽略色);cut2b themed 全锁 + 6 帧对眼。两层快照各锁一层、diff 自动拦漂移。
- **[`AgentEvent` 携 `ToolOutcome` 增大]** → 可接受(单 UI task 消费、非 `Clone`);`ToolOutcome` 已 `Clone`,forward 时 clone 一次。

## Migration Plan

`agent/mod.rs` 加类型 + `run_observed`、`run` 改为委托(行为等价);`tui/*` 扩事件 + observer + 渲染。无数据迁移。回滚 = revert 本 change(`run` 复原内联实现、tui 复原 cut1)。

## Open Questions

- `MaxIterations` / provider 错误路径是否发 `Idle` / 终态事件 —— 实现期以「run 等价 + observer 序列合理」TDD 定;不影响既有行为。
- 工具卡 `output` 多行 / 超长在 transcript 的截断与滚动 —— 滚动属 cut2b;本 change 工具卡 output 先最小呈现。
- cut2b 的 themed 渲染如何吃 `theme` —— `render(frame, state, &Theme)` 形态留 cut2b 定。
