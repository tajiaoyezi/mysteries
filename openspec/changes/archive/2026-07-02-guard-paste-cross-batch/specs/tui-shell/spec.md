## MODIFIED Requirements

### Requirement: 粘贴突发合并输入(批量 drain 防误提交)

TUI 事件循环 SHALL 在每次有 crossterm 事件到达时,用**同步** `event::poll(Duration::ZERO)` + `event::read()` 把当前**已就绪**的事件抽干成一个有界 batch(不 await、不阻塞、与 `EventStream` 共用同一 internal reader 故不污染其 waker),整批处理完只渲染一次;并按"一批**文本内容键**(`Char` + 裸 `Enter`,Press-only)的规模"区分**粘贴突发**与**用户敲击**:突发(批内 ≥2 文本内容键)内的裸 `Enter` SHALL 作换行插入,仅当裸 `Enter` 是其批次里唯一文本内容键时才判提交。

**提交前续读(防跨批/跨周期粘贴误提交)**:因 ConPTY 会把一次大粘贴切成多个 batch 分次投递(`drain` 一次 `poll(ZERO)` 只抽干当前已就绪即停),某换行可能落在此刻 `n==1` 的独立批 → 被误判提交。故 `drain` SHALL 在抽干 `poll(ZERO)` 后,当当前 batch 经 `classify` 将得 `Submit`(落单裸 `Enter`,谓词 `would_submit_lone_enter(batch)` 为真)时,以 `poll(GRACE)`(默认 `PASTE_CONTINUATION_GRACE = 10ms`,具名常量)做一次续读:窗口内有事件 → 读入**同一 batch**,**若读入的是非键盘事件(鼠标 `Moved`/焦点/resize)SHALL 收批**(不无限等待,避免 `EnableMouseCapture` 的高频 `Moved` 令续读不退出、阻塞重绘与 agent 流式),否则回到抽干循环(粘贴续批全是 `Event::Key`,带来 `Char` 后 `n` 变大、谓词转 false、循环终止);窗口内静默 → 收批(真提交)。以 `EVENT_BATCH_CAP` 在**抽干与续读两条路径**封顶防无限续读。续读触发 SHALL 抽为纯函数 `would_submit_lone_enter(batch) = classify_key_batch(press_key_events(batch))` 含 `Submit`;续批读入后 SHALL **复用既有** `classify_key_batch` / `apply_batch_input_key`(裸 `Enter` 因 `n` 变大自然从 `Submit` 变 `Newline`),**不新增 intent 改写逻辑**。此信号取自 `drain` 内同步 `poll`、**不经 `draw` / `select!` / 墙钟**,故不被渲染时延、agent 流式事件、鼠标事件污染。

硬模态在批处理中**逐键按当时活跃态**分治(非整批截断):`pending_permission` 活跃时首键应答后丢弃该批余下键;`models_picker` 活跃时每键透传给 picker(打字过滤/导航/选中 MUST NOT 丢失)。此机制 SHALL NOT 依赖 bracketed paste(Windows crossterm 不产 `Event::Paste`,已诊断探针证实)、SHALL NOT 改 `terminal.rs`、SHALL NOT 改 `select!` 事件循环结构,复用既有文本缓冲的 `InsertNewline`/`InsertStr` 动作与既有 `on_key` 路由。**已知上限(Non-Goal)**:①**大 transcript 慢渲染下正常打字凑批**(末字符 + 提交 `Enter` 落同批 → `n≥2` → `Enter` 误判换行、再按一次即提交;本 requirement 不碰 `classify` 的 `n≥2`→`Newline` 逻辑,该限制原样保留);②粘贴以换行结尾(续读窗口内无续批 → 末 `Enter` 仍提交、不自动换行);③续批间隔慢到 > `GRACE` 的极端(跨秒/极慢分段粘贴)使落单换行仍被判提交;④粘贴含 Tab 丢失;⑤模态关闭后同批粘贴尾 `Enter` 被丢弃——均需更强的到达建模或 bracketed paste(本栈不可用)。

#### Scenario: 粘贴多行整段进入缓冲、不逐行提交

- **WHEN** 一段多行文本被粘贴,产生一批(≥2 个文本内容键)瞬时到达的 `Char`/`Enter` 事件
- **THEN** 该批内所有裸 `Enter` 作为 `InsertNewline` 插入缓冲,连续 `Char` 正文合并为 `InsertStr` 插入缓冲
- **AND** 全程不触发提交,transcript 不新增 user 块,不向 agent 发出 prompt
- **AND** 整批处理完只渲染一次(不逐事件渲染)

#### Scenario: 跨批粘贴续批的落单 Enter 经续读并入同批、判换行不提交

- **WHEN** 一次粘贴被 ConPTY 切成多批分次投递,某个换行此刻落在一个 `n==1` 的独立批里(`would_submit_lone_enter` 为真);其粘贴续批(下一段 `Char`/`Enter`)在 `poll(GRACE)` 续读窗口内到达
- **THEN** `drain` 的续读把续批读入**同一 batch** 并回到抽干循环,该 `Enter` 因 `n` 变大经**既有** `classify_key_batch` 判为 `Newline`、作 `InsertNewline` 不提交(而非因 `n==1` 走 `Submit` 自动发送)

#### Scenario: 续读触发判定为纯函数 would_submit_lone_enter(可单测)

- **WHEN** 对 `would_submit_lone_enter(batch)` 分别给 `[Enter Press]`、`[Enter Press, Enter Release]`、`[Char, Enter]`(n=2)、`[Char]`、空批,以及"`[Enter]` 续读粘入一个 `Char` 后"的批
- **THEN** 落单裸 `Enter`(前两者)→ `true`;`[Char,Enter]`/`[Char]`/空批 → `false`;粘入 `Char` 后 → `false`(经既有 `classify`,`Enter` 因 `n` 变大不再判 `Submit`);该谓词仅由 `press_key_events` + `classify_key_batch` 组合、不碰 `Instant`/IO

#### Scenario: 续读窗口读到非键盘事件即收批(防鼠标 Moved 拖住续读)

- **WHEN** 落单裸 `Enter`(`would_submit_lone_enter` 为真)进入续读,`EnableMouseCapture` 下 `poll(GRACE)` 窗口内读到 `Event::Mouse(Moved)`(或 `Focus`/`Resize`)
- **THEN** 该事件读入同批后 SHALL **立即收批、停止续读**(不因高频 `Moved` 而不退出),`Enter` 交既有 `classify`/`process` 处理;提交至多延迟一个 `GRACE`,重绘与 agent 流式不被阻塞

#### Scenario: 孤立回车经续读确认后仍然提交

- **WHEN** 用户手按一次 `Enter`(该批次里唯一的文本内容键,`modifiers=NONE`),`drain` 续读 `poll(GRACE)` 窗口内终端无紧跟事件
- **THEN** 续读确认无续批后走既有提交路径:trim 后非空则整段作为 prompt 提交、清空缓冲、入历史(手敲后 10ms 内无终端续投,续读不改变提交结果,仅多等一个 `GRACE`)

#### Scenario: Release 事件不计入突发规模

- **WHEN** 用户手按一次 `Enter`,Windows 产生 `[Enter Press, Enter Release]` 两个事件落入同一 batch
- **THEN** 先 `is_key_press` 滤除 Release,批内文本内容键数 `n == 1`,该裸 `Enter` 判为**提交**(而非因 Release 使 n=2 被误判换行)

#### Scenario: 前置守卫消费的键不计入突发规模

- **WHEN** 一批里含被前置守卫消费的键(如 `PageUp`)后紧跟一个裸 `Enter`
- **THEN** `PageUp` 归滚动、不计入文本内容键;`n == 1` → 该 `Enter` 判为**提交**

#### Scenario: 批内带 modifier 的换行键照常换行

- **WHEN** 一批事件里含 `Enter+CONTROL` / `Enter+SHIFT` / `Char('j')+CONTROL`
- **THEN** 这些键按既有 `on_key` 换行分支插入换行,不受"突发 vs 孤立"判定与续读影响(二者只接管落单裸 `Enter`)

#### Scenario: pending_permission 活跃时突发只应答首键

- **WHEN** `pending_permission` 活跃,且一批(≥2 键)突发到达、首键为裸 `Enter`
- **THEN** 首键经 `on_key` 命中权限分支正常应答(Allow),随即**丢弃该批余下键**(一串粘贴 `Enter` 不连答)、**不被降级为换行、不往隐藏缓冲插入杂散 `\n`**

#### Scenario: models_picker 活跃时突发过滤输入不丢失

- **WHEN** `models_picker` 活跃,且一批多个过滤 `Char`(如 "gpt")与导航/选中键同批到达
- **THEN** 每键透传给 `handle_models_picker_key`(`Char`→逐个 `push_filter_char`、`Up/Down`→导航、`Enter`→选中并关闭),**过滤字符全部生效、不被截断丢失**;不套用 burst 换行意图;picker 中途关闭后同批尾随的粘贴裸 `Enter` 被丢弃、不落缓冲/提交

#### Scenario: 软浮层补全不受突发护栏影响

- **WHEN** `command_completion` 浮层活跃,且一批 `Char`/`Backspace` 到达
- **THEN** 逐个改缓冲并重过滤候选(不触发硬模态截断),维持既有 `/` 补全行为

#### Scenario: 退出与中断在批处理中仍即时生效

- **WHEN** 一批事件中出现 `Ctrl+C`(无选区)或运行中的 `Esc`
- **THEN** 既有 `should_exit` / 中断守卫逐键前置生效,命中即退出或中断,不被同批余下事件延迟

#### Scenario: 不依赖 bracketed paste

- **WHEN** 运行于 Windows Terminal(crossterm 不产 `Event::Paste`)
- **THEN** 粘贴合并仅由 batch 突发启发式 + `drain` 内提交前续读实现,`Event::Paste` 分支维持忽略,`terminal.rs` 不变
