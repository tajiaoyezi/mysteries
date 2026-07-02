## Context

`guard-paste-burst-submit` 的 `drain_event_batch(ev0)` 用同步 `poll(ZERO)` + `read()` 把**当前已就绪**事件抽干成一个 batch,`classify_key_batch` 按 Press-only 文本内容键数 `n` 判定:批内 `n≥2` 的裸 `Enter` → `Newline`,`n==1` 的裸 `Enter` → `Submit`。真机诊断探针证明:(1) crossterm 在 WT/ConPTY **不产 `Event::Paste`**(issue #737),bracketed paste 死路;(2) 粘贴换行 `modifiers=NONE`,靠终端标记换行的路死;(3) 一次粘贴被 ConPTY 切成几十个 batch,换行落单成 `len==1` 批时 `n==1` → `Submit` → 误发。

**本 change 的一次方案迭代(记录以免重蹈):** 初版用"墙钟到达间隔 `gap`"——记 `last_batch_end`,落单 Enter 距上批 `gap<20ms` 判续批换行。对抗审查(5 维 + 逐条复核)以真源码否决:`last_batch_end` 在 `draw` 之前更新,`gap` 把整帧重绘时延算进去(finding 1);agent 流式生成时 `select!` 穿插的 `TextDelta` draw 进一步撑大 `gap`(finding 2);`EnableMouseCapture` 的鼠标 Moved 批也刷新 `last_batch_end`,致移鼠标后手敲 Enter 被降级漏提交(finding 3/6)。即**墙钟 `gap` 被 draw / 流式 / 鼠标事件污染,双向失败(既误提交又漏提交)**。结论:判定信号必须取自**终端层的到达连续性**,不能用本进程墙钟。

ratatui `0.29` / crossterm `0.28.1` 的 `poll(timeout)` 可在 `drain` 内做极短阻塞探测,是"终端层信号"的现成手段。

## Goals / Non-Goals

**Goals:**
- 跨批粘贴时落单的裸 `Enter`(粘贴续批换行被切成独立小批)不再被误判提交。
- 手敲 `Enter` 提交行为不变;单批突发判定、带 modifier 换行、非 Enter 键不受影响。
- 判定信号取自终端层(`drain` 内 `poll`),不被 draw / 流式 / 鼠标事件污染。
- 续读触发判定可纯逻辑单测;不新增不可测的 intent 改写逻辑。

**Non-Goals:**
- 不引 bracketed paste、不改 `terminal.rs`、不动 `select!` 事件循环结构。
- 不解决(保留为已知上限):**大 transcript 慢渲染下正常打字凑批**(末字符+提交 Enter 落同批 → `n≥2` → Enter 判换行、再按一次即提交;本 change 不碰 `classify` 的 `n≥2` 逻辑,该限制原样保留);粘贴恰以落单换行收尾且之后无续批(续读窗口静默 → 仍提交);续批间隔慢到 > `GRACE` 的极端(跨秒/极慢分段粘贴);粘贴含 Tab 丢失。

## Decisions

- **D1 `drain` 内"提交前续读"。** `drain_event_batch(ev0)` 改为循环:① `while poll(ZERO) { batch.push(read()) }` 抽干当前就绪(`len≥EVENT_BATCH_CAP` 立即收批);② 若 `!would_submit_lone_enter(&batch)` 或 `len≥CAP` → `break` 收批(正常打字 / `n≥2` 换行 / 已达上限,零续读、零延迟);③ 否则 `poll(GRACE)`:窗口内有事件 → 读一个 `ev`、`batch.push(ev)`;**若 `ev` 非 `Event::Key`(鼠标/焦点/resize)则 `break` 收批**、否则回到 ①(续批粘入后 `n` 变大);窗口内静默 → `break` 收批(真提交)。**终止性**:粘贴续批全是 `Event::Key`(Char/Enter 的 Press/Release),读到 `Char` 后 `would_submit_lone_enter` 转 false → 退出;读到非 `Key` 事件即 `break`;键盘事件的到达间隔受硬件重复率限(≤ ~100Hz,远 > `GRACE`)故 `poll(GRACE)` 自然 timeout——三重保证续读有界,不产生 O(n²)、不无限阻塞。**为何"非 Key 即停"而非"非 Char 即停"**:落单换行的 `Enter Release` 会先于下一行 `Char` 到达,若读到 `Enter Release`(非 Char)就停会把粘贴中段换行误判提交;故键盘事件(含 Release)一律继续续读,只有非键盘事件才停。

- **D2 续读触发 = 纯函数 `would_submit_lone_enter`,复用既有 `classify`。** `pub fn would_submit_lone_enter(batch: &[Event]) -> bool { classify_key_batch(&press_key_events(batch)).contains(&KeyIntent::Submit) }`。因 `Submit` 仅由落单裸 `Enter`(文本内容键 `n==1` 且为裸 Enter)产生,该谓词等价于"这批将提交"。**不做 intent 降级**:续批读入同批后走既有 `classify_key_batch` / `apply_batch_input_key`,Enter 因 `n` 变大自然从 `Submit` 变 `Newline`——无新增改写逻辑、复用已测路径。单测覆盖 `would_submit_lone_enter` 的真值表 + "粘入 `Char` 后转 false"(即 headline 行为的纯逻辑核心)。

- **D3 `GRACE = 10ms` 具名常量。** 依据:ConPTY 连投的粘贴续批间隔在亚毫秒/低毫秒级,`GRACE=10ms` 足以在续批间隙内等到下一段;而人手敲 `Enter` 后 10ms 内不会再有终端数据(下次按键 ≥ 数十 ms)。真机若发现续批偶发 > 10ms(极大粘贴/系统繁忙漏合并)可上调。与早期墙钟阈值不同:`poll(GRACE)` 是**纯终端等待、不含 draw / 流式**,故 10ms 的语义干净(就是"终端静默多久算粘贴结束"),标定余量真实。

- **D4 免疫早期方案的三类污染(审查 finding 1/2/3/6)。** ① **draw(f1)**:`drain` 在 `process_event_batch` 与 `draw` **之前**执行(events 分支拿到 ev0 即 drain),续读是 drain 内同步 `poll`,与整帧 `draw` 无关。② **流式穿插(f2)**:`drain` 在 events 分支内**同步**循环,不 `await`、不进 `select!`、不读 `ui_rx`,故 agent 的 `TextDelta` 不会插进 batch、不影响续读。③ **鼠标/焦点/resize 批(f3/f6)**:不用 `last_batch_end` 墙钟基线;信号是"落单 Enter **之后**终端有无紧跟的**键盘**续批"。移鼠标后手敲 Enter → 续读窗口内无键盘续批 → 照常提交(鼠标事件属 Enter 之前的上一轮 events)。**Enter 之后并发移动鼠标**(mouse capture 高频 Moved 落入续读窗口)→ 续读读到非 `Key` 事件即 `break` 收批(D1 ③),提交至多延迟一个 `GRACE`、不会因 Moved 洪流令续读不退出或 UI 停摆(此点为第二轮审查发现,已并入 D1 终止条件)。④ **可测性(f4)**:无 intent 降级这一新逻辑;续读触发是纯函数 `would_submit_lone_enter`(可测),续批粘入后复用已测 `classify`;仅 `poll(GRACE)` 的 IO 不单测(与既有 `drain` 一致、真机背书)。

## Alternatives considered

- **P1 墙钟到达间隔(`gap` vs 阈值)** —— **被对抗审查以真源码否决**(见 Context):`last_batch_end`/`gap` 被 `draw`、流式 `TextDelta`、鼠标 Moved 批污染,双向失败;且核心 intent 降级内联在不可单测的 `process_event_batch`。放弃。
- **P2 drain 抽干后无条件 `poll(GRACE)` 续读(不看是否将提交)** —— 会给**每次**按键都加 `GRACE` 延迟(正常打字每键多等 10ms)。本 change 的 D1 用 `would_submit_lone_enter` 门控,只在"将提交"时续读,正常打字零延迟。故取 D1 而非无条件续读。
- **bracketed paste(S)/ 靠换行 modifier(B)** —— 探针证实本栈均不可用,排除。

## Risks / Trade-offs

- **`GRACE=10ms` 拍值**:真机若见极大粘贴续批间隔 > 10ms 致漏合并(复发误提交),上调常量;因是纯 `poll` 等待,调参空间干净。
- **粘贴以落单换行收尾**:续读窗口内无续批 → 判提交(保留为 Non-Goal,与既有一致)。
- **提交延迟**:每次落单 Enter 提交多等 1 个 `GRACE`;Windows 下 Enter 的 Press/Release 可能分批到达,续读会多绕一轮(~2×`GRACE`≈20ms),仍无感。
- **Enter 后并发移动鼠标**:续读窗口读到非 `Key` 事件(Moved/Focus/Resize)即收批(D1 ③),提交至多延迟一个 `GRACE`;**不会**因 mouse capture 的连续 Moved 令续读不退出或 UI/agent 流式停摆(第二轮审查发现,已修入终止条件)。
- **模态下续读**:`pending_permission`/`models_picker` 活跃时按 Enter 也会触发续读(`would_submit_lone_enter` 只看键不看模态),但只是让模态应答多等 `GRACE`;续批粘入同批后仍由 `process_event_batch` 既有逐键模态分治处理,语义不变。
- **正常打字凑批(Non-Goal①)**:慢渲染下末字符+Enter 落同批 → `n≥2` → Enter 判换行,本 change 不解决(不碰 `classify`),spec 原样保留该 Non-Goal。
