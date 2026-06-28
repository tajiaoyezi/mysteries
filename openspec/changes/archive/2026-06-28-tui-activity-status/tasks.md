# Tasks — tui-activity-status

> TDD 分界:**强制红-绿** = headless 纯逻辑(① agent-loop `on_usage` 上送 ② `AppState::record_usage` token 累计 + 速率 + `AgentEvent::Usage` 转发);**事后 insta** = 布局 split / 活动行 / 底部 meta 渲染 + 大面快照迁移;**手动** = TUI 冒烟。
> 🔴 **红灯停点**(测试首次成型、贴红灯输出后**停下等确认**再写绿):① `on_usage` observer 上送路径(1.2)② `record_usage` token 累计 / 速率 + `AgentEvent::Usage`(2.2)③ 流式字符估算近似 t/s(2.5)。

## 0. 实施前确认(开 implement 前,先与上游对齐)

- [x] 0.1 上游已拍板 design.md Open Questions(2026-06-28):
  - **Q1** = L1:活动行 / input / 底部 meta
  - **Q2** = 活动行恒占 1 行;idle 显示「上轮 token 摘要 ↓N tok · X t/s」(muted)
  - **Q3** = esc 文案「esc 中断」
  - **Q4** = 仅既有 4 phase(不加 Compacting/思考)
  - **Q5** = approx:完成后真实 t/s + 流式字符估算近似 t/s(标 `~`),完成后真实 usage 校正去 `~`
  - **Q6** = elapsed:UI 侧测 CallingModel→Usage 间隔(Instant 在 IO task、不入 AppState)
  - **Q7** = ↓N tok = 本轮累计 output(TurnComplete/新 Prompt 重置)

## 1. agent-loop — `on_usage` observer 上送(强制 TDD · 🔴)

- [x] 1.1 【红】先只写测,运行确认失败(原因正确非编译错):`RecordingObserver` 覆盖 `on_usage`;Mock 脚本某轮 `ModelResponse.usage = Some(Usage{..})`、另一轮 `None`;`run_observed` 后断言带 usage 轮收到 `on_usage(该 usage)`、`None` 轮不收到;既有 exact-sequence 测(不覆盖 `on_usage`)保持绿。
- [x] 1.2 🔴 **红灯停点①**:贴出 1.1 测试 + 失败输出,**停下等确认**(新 observer 回调路径首次成型),再写绿。
- [x] 1.3 【绿】`AgentObserver` 加 `on_usage(&self, _usage: &Usage)` default no-op;`run_observed` 在 `last_usage = response.usage`(~139)处,若 `Some` 则 `observer.on_usage(&usage)`。`run` 仍 no-op observer。
- [x] 1.4 零回归:既有 agent-loop 测(序列 / run 等价 / 拒绝)+ `run` 逐字节一致保持绿。

## 2. tui — token 累计 / 速率 + `AgentEvent::Usage`(强制 TDD · 🔴)

- [x] 2.1 【红】先只写测:`AppState::record_usage(usage, elapsed)` —— 累计 `↓ N tok`(output 累加)、速率 = `output/elapsed.as_secs_f64()`;`elapsed=0` → 速率 `None` 不 panic;`TurnComplete`/新 prompt 重置累计。`record_usage` 不读系统时钟(注入合成 `Duration`)。运行确认失败。
- [x] 2.2 🔴 **红灯停点②**:贴出 2.1 测试 + 失败输出,**停下等确认**(token 累计 / 速率新逻辑路径 + `AgentEvent::Usage` 首次成型),再写绿。
- [x] 2.3 【绿】`AgentEvent` 加 `Usage{input_tokens,output_tokens}`;`ChannelObserver` impl `on_usage` forward;`AppState` 加 token 累计字段 + `record_usage`(纯逻辑)+ `TurnComplete`/新 prompt 重置;`apply(AgentEvent::Usage)`/`run_tui` 接线(elapsed 按 Q6:UI 侧记 `CallingModel`→`Usage` 间隔,`Instant` 在 IO task、不入 AppState)。
- [x] 2.4 边界(连写):无 usage 轮不显速率;速率 `None` 时活动行只显 `↓ N tok`(或都不显,按 Q2/Q7)。

### 2b. tui — 流式字符估算近似 t/s(Q5 · 强制 TDD · 🔴)

- [x] 2.5 【红】先只写测:纯函数 `estimate_streaming_rate(chars: u32, elapsed: Duration) -> Option<f64>`(或等价)——字符累加经粗估系数(如 chars/4)估 token,速率 = 估 token / elapsed,返回带 `~` 标记的展示值;`elapsed=0` → `None`;真实 `record_usage` 后校正去 `~`。不读系统时钟(elapsed 入参)。运行确认失败。
- [x] 2.6 🔴 **红灯停点③**:贴出 2.5 测试 + 失败输出,**停下等确认**,再写绿。
- [x] 2.7 【绿】实现流式估算 + `on_text` 累加字符 + 活动行显示近似 t/s(标 `~`);`AgentEvent::Usage`/`record_usage` 完成后用真实 usage 校正(去 `~`)。

## 3. tui — 布局 split + 活动行渲染(事后 insta)

- [x] 3.1 `layout_rows`:在 input 上方插入活动状态行(按 Q1 布局序);活动行**恒占 1 行**(Ready/busy 不变高);核对 transcript `Min(8)` 与各 `Length` 不溢出;command completion popup 锚点与活动行协调(不遮挡)。
- [x] 3.2 `render_activity`(新):`{spinner}` + phase 文案(复用 phase→label)+ 运行中 esc 提示(Q3)+ `↓ N tok · X t/s`;Ready 简显(Q2)。配色按 `设计规范/02` 状态机。
- [x] 3.3 `render_status` 改 **meta-only**(去 phase label,置底部状态行)。
- [x] 3.4 事后 insta:活动行(running 固定 spinner_frame / Ready 简显 / 含 token 速率)+ 底部 meta 行 各带色快照;首帧人工对 `设计规范/02` C10 状态机配色审核后锁定。

## 4. 快照迁移 + 回归(布局 split 影响面大)

- [x] 4.1 迁移**几乎所有** `tui_*` 渲染快照(状态行位置变 + 新活动行):`cargo test` 批量更新,**逐帧人工对眼**(welcome / permission / tool card / timeline / scroll / phase lines / fatal error 等),确认仅「活动行 + 底部 meta」结构变化、无非预期回归。
- [x] 4.2 零回归:headless(agent-loop / app 逻辑)测全绿;phase 状态机、msgs 计数语义不变。

## 5. 收尾验证

- [x] 5.1 `cargo build` 通过;`cargo test` 全绿(新红-绿 + 迁移后 insta)。
- [x] 5.2 `openspec validate tui-activity-status --strict` 通过。
- [x] 5.3 TUI 手动冒烟:发消息后输入框上方活动行显示 spinner + phase 变化 + `esc 中断` + `↓ N tok · X t/s`(call 结束刷新);底部状态行只剩 meta;Ready 态活动行简显、布局不跳动。
