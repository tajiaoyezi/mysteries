## Why

当前动态工作状态(phase:调用模型… / 执行 {tool}… / 等待授权…)与静态 meta(provider·model·iter·msgs·cwd)挤在最底同一行(`render_status`,rows[5]),信息密度高、动态状态不醒目、也没有 token 用量反馈。参考 claude code / cursor agent:把**动态工作状态**移到**输入框上方**的「活动状态行」并丰富其变化(spinner + phase 文字 + esc 中断提示 + token 速率),让「Agent 正在做什么、用了多少 token」一眼可见;静态 meta 留在底部状态行。

## What Changes

- **布局 split**:新增**活动状态行**(单行,置于**输入框上方**左上角),承载**动态工作状态**;底部状态行(rows[5])**保留**静态 meta(provider·model·iter·msgs·cwd),**去掉**其 phase label。
- **活动状态多样化**:活动状态行随状态变化呈现 ① **phase 文字**(调用模型… / 执行 {tool}… / 等待授权…,与 `Phase` 枚举对应)② **spinner 动画**(复用 `spinner_frame`,running 态)③ **esc 中断提示**(复用 claude-code-style-loop 的 Esc 中断:运行中显示「esc 中断」)④ **token 速率**(`↓ N tok · X t/s`)。
- **token 用量上送 UI**:`AgentObserver` 增 `on_usage`(default no-op),`run_observed` 每次 `provider.complete` 返回后若 `ModelResponse.usage` 为 `Some` 即上送;`ChannelObserver` forward 为 `AgentEvent::Usage`;`AppState` 累计 token 并算速率(headless 逻辑,强制 TDD)。
- **诚实降级(token 速率)**:经调研,**流式无 token 增量**(wire usage 仅在每次 `complete` 完成后回传,`on_text` 只有文本;项目已拒绝 tokenizer crate)→ **实时滚动 t/s 不可达**。本期 `↓ N tok`(累计,准确)+ `X t/s`(**每次 model 调用完成后**按真实 usage / 该次时长算,准确)。是否要途中「字符估算近似实时数」列为 Open Question,**不硬塞未验证的实时数**(详见 design.md)。

## Capabilities

### New Capabilities
<!-- 无新增 capability:对既有 tui-shell / agent-loop 的 requirement 追加或修订。 -->

### Modified Capabilities
- `tui-shell`:**MODIFIED**「全 phase 状态行 C10」(phase 从底部状态行移到输入框上方的活动状态行)、「状态行常驻 meta」(底部状态行只剩 meta、去掉 phase);**ADDED**「活动状态行(输入框上方)」(spinner + phase 文案 + esc 提示 + token 速率)、「token 用量累计与速率呈现(AppState)」(`AgentEvent::Usage` 转发 + 累计 + 速率,headless TDD)。
- `agent-loop`:**MODIFIED**「结构化观测事件(observer 变体)」(`AgentObserver` 增 `on_usage(&Usage)` default no-op,`run_observed` 每次 model 调用后若有 usage 即上送;`run` 仍 no-op observer、逐字节一致)。

## Impact

- **code**(本轮 propose 不改,仅登记 implement 触及面):
  - `src/agent/mod.rs`:`AgentObserver` 加 `on_usage`(default no-op);`run_observed` 在拿到 `response.usage`(现 `last_usage = response.usage`,~139)后 `observer.on_usage(&usage)`。
  - `src/tui/channel.rs`:`AgentEvent` 加 `Usage { input_tokens, output_tokens }`;`ChannelObserver` impl `on_usage` forward。
  - `src/tui/app.rs`:`AppState` 加 token 累计字段 + `record_usage(output, total, elapsed)`(纯逻辑算速率,**不引 `Instant` 入 AppState**,elapsed 由调用方传入,守住 spinner 「不把时间引入 AppState」契约)+ phase→活动文案映射;`TurnComplete`/新 prompt 重置累计。
  - `src/tui/render.rs`:`layout_rows` 插入活动状态行(输入框上方);新增 `render_activity`;`render_status` 改为 meta-only(去 phase)。command completion popup 锚点(现锚 input 上方)需与新活动行协调。
  - `src/tui/mod.rs`:`run_tui` 把 `AgentEvent::Usage` 喂给 `AppState`;若按推荐方案由 UI 侧测时,记录 `CallingModel`→`Usage` 间隔作 elapsed(`Instant` 在 IO task,不入 AppState)。
- **设计规范引用 + port/adapt/drop**:
  - 活动状态行 / 底部 meta split = **adapt**:`设计规范/02` C10 状态行原为单行(phase 左 + meta 右);本期按 claude-code/cursor 参照 adapt 为「活动行(输入框上方)+ 底部 meta 行」两行,语义保真(phase 状态机配色、meta 字段不变),非像素一致。
  - token 速率 `↓ N tok · X t/s` = **adapt**:`设计规范/` 未定义 token 速率组件,属新增信息(终端版渐进披露);沿用状态机配色语义。
  - esc 中断提示 = **adapt**:沿用 claude-code-style-loop 既有 Esc 中断语义,在活动行新增文字提示。
- **快照面(大)**:布局 split 使**几乎所有 `tui_*` 渲染快照**变化(状态行位置 + 新活动行)→ 大批 insta 迁移、人工对眼(详见 design.md / tasks.md)。
- **deps**:零新增(`std::time` 已可用;不引 tokenizer / unicode-width)。
