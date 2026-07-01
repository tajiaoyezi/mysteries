## Why

粘贴多行内容会**逐行自动提交**:粘贴的内部换行在 Windows 上以 `Enter modifiers=NONE` 的按键事件到达,与用户手按的提交 `Enter` **字节级无法区分**;而事件循环**逐事件处理、每个事件后 `terminal.draw()`**(mod.rs 主循环 L204),一波粘贴 = 一串 `Char`/`Enter` 事件被当成"打字 + 多次提交",把每段当独立 prompt 发出去。

这是 `add-multiline-input` 拆分时**刻意留给本 change** 的一块:它需要把事件循环改成**批量 drain**才能既避免 render 耗时污染时序判定、又能把"一波瞬时到达的事件"整体识别为粘贴。本 change 建在 `add-multiline-input` 的文本缓冲之上。

## What Changes

- **批量 drain 事件循环(同步 poll)**:某个 crossterm 事件从异步 `events.next()`(真 tokio waker)到达后,用**同步** `crossterm::event::poll(Duration::ZERO)` + `event::read()` 把当前**已缓冲**的事件抽干成一个有界 batch(`CAP` 兜底),整批处理完**只 `terminal.draw()` 一次**。**不用 `EventStream::next().now_or_never()`**——它的 noop-waker 会毒化 crossterm 的 wake-task 注册,使输入被 120ms spinner 心跳量化成卡顿(已对 crossterm 0.28.1 源码验实);同步 `poll/read` 与 `EventStream` 共用同一 internal reader、不碰 waker,无此副作用。
- **突发 → 换行 判定(纯逻辑)**:先 `is_key_press` **滤掉 Release**(Windows 每键 Press+Release 双发,孤立 Enter=2 事件,若不滤则突发数翻倍、提交永久失效),再以**落到输入层的文本内容键(`Char` + 裸 `Enter`)** 数 `n`(被前置守卫消费的滚动/退出/方向键不计入)。`n >= 2`(突发)时批内裸 `Enter` 当**换行**、不提交;仅当裸 `Enter` 是其批次里**唯一文本键**(`n == 1`,孤立敲击)才**提交**。
- **硬模态突发护栏(逐键按活跃态,pending 与 picker 分治)**:`pending_permission` 活跃时首键应答后**丢弃该批余下键**(防一串 `Enter` 连答);`models_picker` 是**打字过滤/导航的多键面**,活跃时**每键透传给 picker**(过滤字符 MUST NOT 被截断丢失),仅在 picker 中途关闭后丢弃同批尾随的粘贴裸 `Enter`。硬模态活跃的键一律交 `on_key`、不套 burst 换行意图(防幸存裸 Enter 被误降级为换行绕过模态)。软浮层 `command_completion` 同样逐键透传、维持过滤。
- **换行意图复用 on_key 副作用**:批内突发换行 → `AppState::insert_newline_and_refresh()`(插 `\n` + `refresh_command_completion()`),**与 on_key 换行分支共用同一实现**,防补全刷新等副作用两处漂移;粘贴正文的**连续普通 `Char` 合并成一次 `InsertStr`**(见下)。
- **给文本缓冲加 `InsertStr(String)`**:reducer 每 action `state.clone()`(含整个 `input_history`),逐字符 `InsertChar` 对大粘贴是 **O(n²)**;批处理把连续 Passthrough Char 攒成一次 `InsertStr` 插入(一次 clone),把 clone 降到 ~O(n)。(推翻 `add-multiline-input` 当时"不引 InsertStr"的暂缓——它正是留待本 change。)
- **不引 bracketed paste**:Windows crossterm 从不产 `Event::Paste`,故不 `EnableBracketedPaste`、不改 `terminal.rs`;`Event::Paste` 分支维持忽略。

## Capabilities

### Modified Capabilities

- `tui-shell`:
  - **ADDED**:`粘贴突发合并输入(批量 drain 防误提交)` —— 同步 poll 批量 drain + 整批单次渲染、Press-only 文本键计数、突发内裸 Enter 归换行 / 孤立才提交、硬模态先截断再分类、InsertStr 合并粘贴正文、无 bracketed paste 的启发式。
  - **MODIFIED**:`多行输入编辑(文本缓冲 + 光标 + 换行)` —— ①把"`Enter`(无 CONTROL/SHIFT)SHALL 提交"改为**附条件**:孤立敲击(批内唯一文本键)才提交、粘贴突发批内作 `InsertNewline`(交叉引用「粘贴突发合并输入」);②动作集**加入 `InsertStr`**(原文注明"不含 InsertStr——属后续 change"随之更新)。

## Impact

- **依赖**:**无新增**(`crossterm::event::poll/read` 已在用;不引 `futures::now_or_never`)。
- **代码**:
  - `src/tui/`:新增纯逻辑批次分类模块(`press_key_events` 滤 Release、`classify_key_batch`、`hard_modal_key_limit`);`input_buffer.rs` 加 `InsertStr` 动作;`app.rs` 加 `insert_newline_and_refresh()`(与 on_key 换行分支共用)与批 apply 入口。
  - `src/tui/mod.rs`:主循环 `events.next()` 臂改为同步 `poll(ZERO)`+`read` 抽干成 batch → 逐事件分派(Key 走意图,硬模态先截断;Mouse/Resize 照旧)→ 整批 draw 一次。
  - `src/tui/terminal.rs`:**不改**。
- **测试**:`press_key_events`/`classify_key_batch`/`hard_modal_key_limit`/`InsertStr` reducer 纯逻辑 TDD;app 层投递意图序列单测(整批不提交、completion 经 Newline 关闭、硬模态首键得 Allow);批量 drain 接线属 TUI 外壳,既有 render 快照不churn + 真机复核。
- **风险 / Non-Goals(启发式固有上限,见 design D8)**:慢/跨周期粘贴、飞快打字凑批、粘贴以换行结尾、粘贴含 Tab 丢失——均对称记为已知上限,不在本 change 解决(需真实到达时序或 bracketed paste)。批量 drain 不得饿死 `ui_rx`/`spinner_tick`(只抽已就绪、单次有界)。
- **前置**:建在 `add-multiline-input`(已 archive)之上。
