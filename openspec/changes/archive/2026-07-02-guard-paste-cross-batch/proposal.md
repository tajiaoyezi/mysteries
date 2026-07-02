## Why

`guard-paste-burst-submit` 用**单批** batch-size 启发式防粘贴狂发:一批里裸 `Enter` 若非唯一文本内容键(`n≥2`)则作换行、不提交。但真机复现 + 诊断探针证明:**ConPTY 会把一次大粘贴切成几十个 batch 分次投递**(日志见 `--- batch len=2/10/26/34/60/102 --- ...` 满屏),`drain_event_batch` 一次 `poll(ZERO)` 只抽干"当前已就绪"的事件即停,导致某个换行可能落在一个 `len==1` 的独立批里 → `classify` 得 `n==1` → 判 `Submit` → **自动发送**。行数越多、切批越多,落单概率越高——正是用户"粘贴行数过多就自动发送"的根因。这是 `guard-paste-burst-submit` 决策记录里 D8 明确记的 Non-Goal(需"真实到达时序或 bracketed paste")。

标准解法 bracketed paste **在本栈不可用**:诊断探针启用 `EnableBracketedPaste` 后,Windows Terminal + crossterm 0.28.1 下**仍不产 `Event::Paste`**(全日志无 `event=paste`,证实 crossterm issue #737),粘贴换行也**不带任何 modifier**(所有 Enter 行 `modifiers=NONE`,#962 现象不出现)。

区分"粘贴续批的落单 Enter" vs "手敲提交 Enter"的可靠信号只能是**终端层的到达连续性**:粘贴续批由 ConPTY 连投、间隔在亚毫秒/低毫秒级;人手敲 `Enter` 后不会立刻再有终端数据。故:**在 `drain` 内,当一批将判提交(落单裸 Enter)时,给一个极短的续读窗口 `poll(GRACE)` 看终端是否紧跟着还有事件——有则说明是粘贴续批、把它读进同一批(Enter 前后有了内容、`n` 自然变大 → 既有 `classify` 判换行);无则确认是真提交。** 判定信号取自 `drain` 内的同步 `poll`,不经 `draw` / `select!` / 墙钟,故不被渲染时延、流式事件、鼠标事件污染(此点是相对早期"墙钟 gap"方案的关键修正,详见 design)。

## What Changes

- **`drain_event_batch` 加"提交前续读"**:抽干 `poll(ZERO)` 后,若当前 batch 经 `classify` 会得 `Submit`(落单裸 `Enter`),则 `poll(GRACE)`(默认 10ms)续读:窗口内有事件 → 读入同批、回到抽干循环(续批粘入后 `n` 变大、不再判 `Submit`);窗口内静默 → 收批(真提交)。以 `EVENT_BATCH_CAP` 封顶防无限续读。
- **续读触发判定做成纯函数**:`would_submit_lone_enter(batch: &[Event]) -> bool`(= `classify_key_batch(&press_key_events(batch))` 含 `Submit`),可单测。
- **不做 intent 降级**:续批被读入同一 batch 后,走**既有** `classify_key_batch` / `apply_batch_input_key`——落单 Enter 因 `n` 变大自然从 `Submit` 变 `Newline`,无新增的"改写 intent"逻辑。
- **不引墙钟时序**:不记 `last_batch_end`、不算批间 `gap`;只用 `drain` 内 `poll` 的续读窗口。
- **收窄 D8 Non-Goal**:"跨批/跨周期粘贴换行落单被误判提交"由续读解决、移出 Non-Goal;**保留**主 spec 既有 Non-Goal①(大 transcript 慢渲染下正常打字凑批,`n≥2` 使 Enter 误判换行——本 change 不碰 `classify` 的 `n≥2` 逻辑,该限制仍在)、以及粘贴以换行结尾(续读窗口内无续批 → 仍提交)、续批间隔慢到 > `GRACE` 的极端、粘贴含 Tab。

**不改的**:`terminal.rs` 不动(不引 bracketed paste)、`classify_key_batch` 语义不动、`InsertNewline`/`InsertStr`/`on_key` 路由不动、事件循环 `select!` 结构不动。续读是**加在** `drain` 抽干后的一道确认,不重写既有 batch 机制。

## Capabilities

### Modified Capabilities

- `tui-shell`:
  - **MODIFIED**:`粘贴突发合并输入(批量 drain 防误提交)` —— 在既有单批 batch-size 启发式上叠加 `drain` 内"提交前续读":落单裸 `Enter` 提交前 `poll(GRACE)` 确认终端无紧跟续批,有续批则读入同批(经既有 `classify` 判换行)、无则提交;收窄 D8 中"跨批粘贴换行落单误提交",保留其余已知上限。

## Impact

- **代码**:
  - `src/tui/input_batch.rs`:加纯函数 `would_submit_lone_enter(batch: &[Event]) -> bool` + 单测(落单 `[Enter]`/`[Enter Press,Enter Release]` → true;`[Char,Enter]`(n=2) → false;`[Char]`/空批 → false;`[Enter]` 粘入一个 `Char` 后 → false)。
  - `src/tui/mod.rs`:`drain_event_batch` 改为"抽干 → 若 `would_submit_lone_enter` 则 `poll(GRACE)` 续读 → 有则读入回到抽干、无则收批"的循环;加具名常量 `const PASTE_CONTINUATION_GRACE: Duration = Duration::from_millis(10)`;沿用 `EVENT_BATCH_CAP` 封顶。`process_event_batch` / `apply_batch_input_key` **不改**。
- **依赖**:零新增(`std::time::Duration` + 既有 `crossterm::event::poll`)。
- **测试**:`would_submit_lone_enter` 纯函数走单测(含"续批粘入后不再判提交"经既有 `classify` 的行为);`drain` 内 `poll(GRACE)` 续读的 IO 行为不单测(与既有 `drain_event_batch` 一致,由真机复核背书,符合 CLAUDE.md「TUI 外壳交互事后回归」边界)。真机复核:粘贴 20+ 行**不再自动发送**、手敲孤立 `Enter` 仍提交(多等 ~10ms 无感)、agent 流式生成时粘贴多行不误发。
- **风险 / 取舍**:见 design。核心:`GRACE=10ms` 是拍值(真机可调),但比早期墙钟方案鲁棒得多(纯 `poll` 等待、不含 `draw`);"粘贴恰以落单换行收尾且之后无续批"仍会提交(与既有 Non-Goal 一致);每次落单 Enter 提交多等 1~2 个 `GRACE`(Release 分批时),~10–20ms、无感。
