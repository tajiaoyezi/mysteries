## Context

现状(已对 HEAD=1b43785 代码核实):

- **布局**(`render.rs::layout_rows` ~41 + `render` ~14):rows[0] header(3)/ rows[1] transcript(Min 8)/ rows[2] permission / rows[3] gap / **rows[4] input(3)** / **rows[5] status(1,最底)**。`tui-ux-and-cli-auth` 已把状态行移到**输入框下方**最底一行;`render_status`(~866)同一行内左侧 phase label(`◇ 就绪`/`{spinner} 调用模型…`/`{spinner} 执行 {tool}…`/`▲ 等待授权…`)+ 右侧 `status_meta`(~905:`provider · model · iter X/maxIter · N msgs · cwd`)。command completion popup 锚在 input 上方(`render_command_completion` ~988)。
- **phase 来源**:`AppState.phase`(`app.rs` `Phase`:`Ready`/`Busy`/`CallingModel`/`ExecutingTool(String)`/`WaitingForPermission`),由 `AgentEvent::StatusChanged` 驱动;`AgentStatus`(`agent/mod.rs:43`)只有 `Idle`/`CallingModel`/`ExecutingTool`/`WaitingForPermission`——**无 `Compacting`**;自动压缩在 `strategy.prepare`(loop 内)发生、**不发 status**。
- **spinner**:`AppState.spinner_frame` + `advance_spinner`,render 仅依据 `spinner_frame`(既有「spinner 确定性渲染」requirement:**不把时间引入 render / AppState**)。
- **token 数据**:`ModelResponse.usage: Option<Usage{input_tokens,output_tokens}>`(`provider/mod.rs:41`),provider **完成后**回传(OpenAI 末尾 usage-only chunk;Anthropic `message_delta` 终值)。`run_observed`(`agent/mod.rs:139`)每次 `complete` 后 `last_usage = response.usage` —— **每次 model 调用后**可拿该轮 usage。`DeltaSink::on_text` 只给**文本增量、无 token 计数**。`AgentObserver`(`agent/mod.rs:50`)有 `on_status`/`on_tool_call_started`/`on_tool_call_finished`(全 default no-op),**无 `on_usage`**;`AgentEvent`(`tui/channel.rs`)**不转发 usage**;`AppState` **无任何 token 字段**。

约束(CLAUDE.md):纯 Rust、不擅扩 dependency(**已拒绝 tokenizer crate**,见 expose-token-usage);headless 内核(token 累加 / 速率)强制 TDD;TUI 渲染事后 insta;视觉以 `设计规范/` 为准、行为以 code/test 为准。

## Goals / Non-Goals

**Goals:**
- 动态工作状态(spinner + phase 文字 + esc 提示 + token 速率)移到**输入框上方**的活动状态行;底部状态行只留静态 meta。
- token 用量经 observer 上送 UI、累计并(尽可能准确地)给出 `↓ N tok · X t/s`。
- 给出 token 速率**数据可达程度的诚实结论**与降级路径。

**Non-Goals(本期不做):**
- **实时滚动 t/s**(流式途中逐 token 计数):数据不可得(见决策 ③),不做;不引 tokenizer 估算 token。
- **`Compacting` / `思考` 新 phase**:当前 `AgentStatus` 无对应、自动压缩不发 status;本期用既有 4 phase 的文案,新 phase 列 Open Question(决策 ②)。
- 不改 agent-loop 的终止 / 错误 / history 契约(仅 observer **加** default no-op 方法)。
- 不引 `Instant` 入 `AppState` / `render`(守 spinner 确定性契约,决策 ④)。

## Decisions

### 决策 ① 布局 split:活动行(输入框上方)+ 底部 meta 行

新 `layout_rows`(在 input 之上插入活动行):header / transcript / permission / gap / **活动状态行(1)** / **input(3)** / **底部 meta 行(1)**。

- 活动状态行 = 动态:`{spinner} {phase 文案}` + 右/尾 `↓ N tok · X t/s` + 运行中 ` · esc 中断`。
- 底部 meta 行 = `render_status` 改为 **meta-only**(`provider · model · iter X/maxIter · N msgs · cwd`,去 phase label)。
- **布局序待拍板(Open Questions Q1)**:
  - **L1(推荐)**:`… / 活动行 / input / 底部 meta`(活动行紧贴 input 上沿=claude-code「左上角」;meta 在最底=「保留底部状态行」字面)。
  - **L2**:`… / 活动行 / 底部 meta / input`(meta 维持现「input 上方」位、活动行再上一行)——活动行不紧贴 input。
  - 推荐 L1(最贴近 claude-code/cursor 参照 + 上游「输入框上方左上角」「底部状态行」双描述)。

**Ready/Idle 态(Open Questions Q2)**:为**避免布局跳动**(行高变化致 transcript reflow),活动行**恒占 1 行**(不在 idle 折叠为 0)。idle 内容推荐:**静默简显**——若刚结束一轮则保留该轮 `↓ N tok · X t/s` 摘要(muted),否则空 / `◇ 就绪`。具体 idle 文案交上游。

**command completion popup**:现锚 input 上方(`y = input.y - height - 1`),与新活动行同区;implement 需让 popup 盖在活动行之上或上移,避免遮挡(事后 insta 核对)。

### 决策 ② 活动状态文案多样 + esc 提示 + 新 phase

- **phase→文案**(与 `Phase` 枚举对应):`CallingModel`→`{spinner} 调用模型…`、`ExecutingTool(name)`→`{spinner} 执行 {name}…`、`WaitingForPermission`→`▲ 等待授权…`、`Busy`→`{spinner} 处理…`、`Ready`→idle(见 Q2)。配色沿用 `设计规范/02` 状态机(accent.primary / warning.fg)。
- **esc 中断提示**:运行中(phase 非 Ready/WaitingForPermission)活动行尾加 ` · esc 中断`(复用 claude-code-style-loop 的 Esc 运行中中断;`WaitingForPermission` 时 esc=拒绝,由权限框管,不在此重复)。**文案待拍板**(Open Questions Q3):`esc 中断` / `esc 停止` / `(esc 中断本轮)`。
- **`思考` / `Compacting`(Open Questions Q4)**:当前无对应 `AgentStatus`。选项:(a)本期不加,仅用既有 4 phase(推荐,最小);(b)加 `AgentStatus::Compacting` + `Phase::Compacting`,在自动压缩(`strategy.prepare`)/ `/compact` 时发 status → 活动行 `压缩上下文…`;`思考` 可作 `CallingModel` 首 token 前的文案变体。(b) 扩 agent-loop + 压缩路径,较重,交上游定是否本期纳入。

### 决策 ③ token 速率数据来源:诚实可达程度 + 流式近似(Q5 拍板)

**调研结论**:**流式无 token 增量** —— provider 的 `usage` 只在每次 `complete()` **完成后**回传;`DeltaSink::on_text` 只有文本增量、**不带 token 计数**;项目**已拒绝 tokenizer crate**。故「流式 completion token 累加」**做不到精确值**。

**可达程度**:
- ✅ **`↓ N tok`(累计,准确)**:累加每次 `complete` 的真实 `usage.output_tokens`。
- ✅ **`X t/s`(每次 model 调用完成后,准确)**:真实 `output_tokens / elapsed`,call 结束刷新。
- ⚠️ **流式近似 t/s(标 `~`)**:流式 `on_text` 累加字符 → 粗估 token(如 `chars/4`,**无 tokenizer、必标 `~` 不误导为精确**)→ 近似 `~X t/s`;**完成后**真实 `on_usage` 校正,去 `~` 换真实速率。

**方案 + 取舍**:
| 方案 | 做法 | 结论 |
| --- | --- | --- |
| **A 完成后真实速率** | `on_usage` 上送;`record_usage` 累计 + 真实 t/s | **采纳 ✅** |
| **B 字符估算近似实时** | `on_text` 累字符/elapsed 粗估 tok/s,标 `~`;完成后 A 校正 | **采纳 ✅(Q5,标 ~ + 完成后校正)** |
| **C 实时精确流式 t/s** | 需 tokenizer / 流式 usage | **不做 ❌** |

### 决策 ④ 速率计时不入 AppState(守 spinner 确定性契约)+ elapsed 来源

- `AppState::record_usage(output_tokens, total_tokens, elapsed: Duration)` 为**纯函数**:累计 token、`last_rate = output as f64 / elapsed.as_secs_f64()`(elapsed 为 0 时不算/置 None)。**不**在 `AppState`/`render` 持 `Instant`(守既有 spinner「不把时间引入 render/AppState」requirement,且 TDD 可注入合成 `Duration`)。这是 token 累计/速率的**强制 TDD** 单元(🔴 红灯停点)。
- **elapsed 来源(Open Questions Q6)**:
  - **推荐**:`on_usage(&Usage)` 仅带 usage(agent-loop 最小);**`run_tui`(IO task)记录 `StatusChanged(CallingModel)` 与随后 `Usage` 的间隔**作 elapsed(`Instant` 在 IO task,不入 AppState),传给 `record_usage`。速率含极小 channel 延迟,可接受。
  - **备选**:`run_observed` 包住 `complete()` 测 per-call 时长,经 `on_usage(usage, elapsed)` / `AgentEvent::Usage{.., elapsed}` 上送(更准,排除 channel 延迟;代价 = observer 契约带 `Duration`)。
- **`AgentEvent::Usage` 形状**:`{ input_tokens: u32, output_tokens: u32 }`(推荐方案下不带 elapsed)。`N tok` 取**本轮累计 output**(`TurnComplete`/新 prompt 重置,与 `iteration` 一致语义);session 总量 / `total()` 列 Open Questions Q7。

### 决策 ⑤ spec 挂载

- `agent-loop`:**MODIFY**「结构化观测事件(observer 变体)」—— 加 `on_usage(&Usage)` default no-op + `run_observed` 每次 model 调用后若 `usage` 为 `Some` 上送;`run` 仍 no-op observer、逐字节一致(既有 3 scenario 保留,加 1 个 usage scenario)。因 `on_usage` default no-op 且既有 `RecordingObserver` 不覆盖它,既有 exact-sequence 测试不破。
- `tui-shell`:**MODIFY**「全 phase 状态行 C10」(phase 移活动行)、「状态行常驻 meta」(底部行 meta-only);**ADD**「活动状态行(输入框上方)」、「token 用量累计与速率呈现」(`AgentEvent::Usage` + `record_usage` + 活动行 `↓ N tok · X t/s`)。

## Risks / Trade-offs

- **快照迁移面大** → 布局 split 使几乎所有 `tui_*` 渲染快照变化(状态行位置 + 新活动行)。implement 跑 `cargo test` 批量更新、**人工对眼**(首帧对 `设计规范/02` C10 状态机配色)。属事后 insta,不走 red-green。
- **t/s 非实时,用户可能期待「跳动」** → 诚实取舍(决策 ③):准确优先;若上游要观感,Q5 的字符估算可后加(标近似)。**不**承诺流式实时 t/s。
- **速率含 channel 延迟(推荐 elapsed 方案)** → 延迟为 ms 级,对 t/s 显示影响可忽略;要更准用决策 ④ 备选(per-call 计时)。
- **活动行恒占 1 行**占用一行高度 → 视口少 1 行;换来布局稳定(无 idle/busy 跳动),值得。
- **observer 加方法** → default no-op,既有调用方与 `run` 零负担、零回归(spec 既有 scenario 保留)。

## Open Questions(交上游拍板)

- **Q1 布局序**:L1(活动行/input/底部 meta,推荐)还是 L2(活动行/底部 meta/input)?
- **Q2 Ready/Idle 活动行**:恒占 1 行(推荐,防跳动)?idle 显「上轮 token 摘要」/ 空 / `◇ 就绪`?
- **Q3 esc 提示文案**:`esc 中断` / `esc 停止` / `(esc 中断本轮)`?
- **Q4 新 phase**:本期是否加 `Compacting`(+`思考`)phase(扩 agent-loop + 压缩路径发 status),还是仅用既有 4 phase?(推荐:本期仅既有 4 phase)
- **Q5 近似实时速率**:✅ **采纳** —— 流式字符粗估近似 t/s(标 `~`),完成后真实 usage 校正(去 `~`)。
- **Q6 elapsed 来源**:UI 侧测 `CallingModel`→`Usage` 间隔(推荐,agent-loop 最小)还是 `run_observed` per-call 计时(更准、observer 带 `Duration`)?
- **Q7 `N tok` 口径**:本轮累计 output(推荐)/ session 总量 / `total()`(input+output)?
