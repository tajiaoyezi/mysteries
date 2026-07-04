# tui-shell Delta

## ADDED Requirements

### Requirement: 剪贴板校准粘贴快路径(大粘贴即时折叠与尾流校准丢弃)

TUI SHALL 为大段粘贴提供**剪贴板校准快路径**:事件流仅作「发生了粘贴」的信号,折叠内容取剪贴板原文——命中时 MUST 立即折叠入框(不等 ConPTY 投递完整段),其后仍在到达的事件尾流 MUST 按**期望内容逐事件校准**:匹配即丢弃、失配即转发、内容耗尽即精确清态。未命中时 MUST 回退既有慢路径(合批 + `fold_candidate`),除**一次剪贴板惰性读取与一帧接收提示**外行为与无本 requirement 时一致。

**判定(纯函数,惰性读取,门槛按序短路)**:`try_fast_paste(batch, read_clipboard) -> Option<FastPaste { fold_text, tail }>`,`read_clipboard: FnOnce() -> Result<String, String>` 惰性注入。调用方前置门 = 无 `pending_permission`、无 `models_picker`、`batch.len() >= PASTE_COALESCE_MIN_EVENTS`;函数内按序:① 批内 press 键全为**可重建键**(`Char` 非纯 Ctrl、裸 `Enter`、`Tab`;重建映射 `Char`→字符、`Enter`→`\n`、`Tab`→`\t`)否则 `None`;② 重建文本 `chars().count() >= PASTE_FAST_MIN_MATCH_CHARS`(=8,具名常量)否则 `None`——**此门 MUST 先于 `read_clipboard` 调用**(短打字/IME 短句突发不触发剪贴板读取,以 mock 调用计数可测)。批像粘贴但重建不足 8 字符时,调用方 MUST 先做**预试凑批(top-up)**:以 `PASTE_COALESCE_GRACE` 最多 `PASTE_FAST_TOPUP_ROUNDS`(=5,具名常量)轮 grace 凑批(读入回抽干、非 key 收批,与桥接同规),凑满 ≥8 字符或静默/收批即止,再对累积批**恰一次**尝试;仍未命中 → 累积批交桥接续走慢路,无重复消费(首批常仅数条记录,不凑批则快路径对真实大粘贴永不咬合——真机 1080 行 6s 坐实);③ `read_clipboard()` 失败或全空白 → `None`;剪贴板经**换行归一**(`\r\n`→`\n`、孤 `\r`→`\n`);④ **前缀匹配(先于阈值全扫)**:以 `PasteTailMatcher`(见下)从归一化剪贴板头部逐事件推进批的 press 键,MUST **全部匹配(零转发)**,任一失配 → `None`(非粘贴突发在此快拒,不付全文扫描);⑤ 归一化剪贴板 MUST 满足折叠阈值(`行数 >= PASTE_FOLD_MIN_LINES || 字符数 >= PASTE_FOLD_MIN_CHARS`,与慢路同常量)否则 `None`。命中:`fold_text` = 归一化原文,MUST 经既有 `insert_paste_fold` 入口插入(存储/渲染/编辑/提交语义悉从「粘贴折叠占位符」requirement;行数为 `split('\n')` 口径,尾随换行计入 +1,与该 requirement 一致);`tail` = 已步进到批末匹配位置的 matcher;命中批 MUST NOT 再入 `process_event_batch`(不重复插入)。

**尾流校准丢弃(`PasteTailMatcher`,换行 run 感知纯状态机)**:持归一化剪贴板与匹配游标,事件分四类处置——**非 key 事件** MUST 转发且 MUST NOT 触碰 matcher 状态(滚轮/拖选/焦点/resize 照常,不退出吸收态);**key Release** MUST 丢弃(内容无关,不触碰状态);**非可重建键 press**(纯 Ctrl 组合如 `Ctrl+C`、`Esc`、方向/功能键)MUST 恒转发且 MUST NOT 触碰状态(退出/中断/选区复制**结构性免吞**,内容匹配对其无定义);**可重建键 press**(`Char` 非纯 Ctrl、裸 `Enter`、`Tab`)按期望推进——期望为 `\n` run 起点且事件为裸 `Enter` → 越过整个 run 并进入吸收态、丢弃(吸收态内后续紧邻 `Enter` 丢弃不推进,容「每换行 1 或 2 个 `Enter`」两种到达形态;**可重建的非 `Enter` 键**到达才退出吸收态);期望字符与事件重建字符相等 → 丢弃、推进;失配且期望为**流侧不可靠字符**(astral 码点 `> U+FFFF`、`U+FE0F`、`U+200D`——ConPTY 可能整个吞掉不投递)→ MUST 跳过期望侧连续不可靠字符后对同一事件重试一次(命中 → 丢弃推进,零泄漏;终端真投递 astral 时正常匹配分支先命中不触发跳过);**失配 → 转发该事件**(粘贴后打字 / 模态应答 MUST 经转发直通既有处理),转发后对同一游标继续重试(尾流恢复匹配即恢复丢弃)。**终止三层(吸收先于 done)**:游标达内容末尾**且不在吸收态** → 精确清态(不依赖时钟,慢帧免疫),批内余下事件转发——游标经**尾部 `\n` run** 越至末尾时 MUST 保持吸收态继续吞紧邻 `Enter`(否则源以换行结尾 × 双 `Enter` 形态下吸收残余被转发成 lone Enter 误提交),直到可重建非 `Enter` 键到达(清态并转发该键)或兜底超时;连续失配转发 `>= PASTE_TAIL_ABORT_MISMATCHES`(=16,具名常量;任一匹配将 streak 归零)→ 进入 **aborted 护栏态**(内容跟踪失效,如剪贴板被改写或字符被终端改形):可重建键 MUST 恒丢弃(洪流不入输入框、不再内容比对)、非可重建键与非 key MUST 恒转发(`Esc`/`Ctrl+C`/滚轮直通),`is_done()` MUST 保持 false,由 2s 静默兜底收场(丢弃刷新计时);中止时 debug 模式 SHALL 记 `paste-tail abort streak=16 cursor=N normalized_len=M`(不含内容);key 静默 `>= PASTE_TAIL_QUIET_FALLBACK`(=2s,具名常量;仅匹配丢弃的 key 刷新计时)→ 兜底清态(尾流被截断/尾部吸收态悬置时提示不滞留),判定点 MUST 覆盖「事件批处理前」与「既有 spinner tick」两处。命中时 matcher 已耗尽(整段落入首批,无尾流)则 MUST NOT 置尾流态(提示不空挂)。

**提示**:`paste_tail` 活跃期,活动行右侧 SHALL 显示「⋯ 接收粘贴」轻提示(`text_muted`,复用复制轻提示的右对齐渲染位);**与 copy_hint 同时活跃时 copy_hint 优先**(动作反馈优先,4s TTL 过期后本提示恢复显示;「复制成功轻提示」requirement 不受本 requirement 改写)。快路径**未命中**且批像粘贴(≥ `PASTE_COALESCE_MIN_EVENTS`)时,SHALL 在进入阻塞合批**前**渲染一帧同款提示(阻塞期用户可见状态;该帧为「粘贴突发合并输入」的「整批处理完只渲染一次」的显式例外),批处理完成后随后续帧消失。

**观测与隐私**:剪贴板读取 MUST 仅发生于粘贴样突发判定内;剪贴板内容 MUST NOT 进入任何日志。被快路径消费与尾流**丢弃**的事件(二者不经 `process_event_batch`)SHALL 由 run_tui 层写 `MYSTERIES_TUI_DEBUG_EVENTS` 事件日志(既有 redact 形态不变),行尾分别加 ` disposition=fast-paste` / ` disposition=tail-drop` 标记;尾流**转发**事件 MUST NOT 在 run_tui 层重复记录(由既有 `process_event_batch` 顶部日志自然记录,无标记、夹在 `tail-drop` 行间可辨),同一事件不双行。快路径尝试未命中时 SHALL 记一行 `paste-fast decline reason=<too-short|no-match|clipboard-err|below-threshold> rebuilt_chars=N batch_len=N`;matcher 中止时 SHALL 记一行 `paste-tail abort streak=16 cursor=N normalized_len=M`(均不含任何内容字符,可真机定位)。

**Non-Goals(v1,如实边界)**:模态应答键(可重建 `Char`)恰与尾流期望字符相等时该次被吞(游标推进后经一次失配转发自动重对齐,泄漏字符恒等于用户所敲,自愈;控制键因非可重建而结构性免疫);二次粘贴落在首段尾流未耗尽前 → 其事件大概率失配转发、以普通输入涌入(现象同今日粘贴中再粘,不新增劣化;尾流耗尽后二次粘贴照常);中止(streak 16)护栏态期间用户可重建键与洪流一并被吞(≤ 洪流余量 + 2s 静默,有提示;非可重建键直通);吸收态被批间隙的用户**可重建键**打断(须恰落 CRLF 双 `Enter` 对的 ~8ms 间隙且在 chunk 边界)时残余 `Enter` 失配转发、可能成为换行或 lone Enter 提交——三重小概率,接受并由 disposition 日志可观测;前缀假阳性(≥8 字符突发恰为剪贴板头部,如 IME 整句提交撞上自己刚复制的草稿开头)的**主代价是剪贴板全文被误折入框**——fold 原子、单次 `Backspace` 可删,概率极低,接受;首批不足 `PASTE_COALESCE_MIN_EVENTS`(1 字符级小片)时该字符先按打字入框、其后批因前缀失配退慢路(无提速、不劣化;偏移匹配回删已弃,维持);粘贴样批被剪贴板管理器在 ~ms 窗口内改写且保持前缀一致时 fold 内容以剪贴板为准(「内容取剪贴板」的固有信任边界)。

#### Scenario: 大粘贴命中快路径即时折叠

- **WHEN** 剪贴板持 610 行文本(归一后达折叠阈值),粘贴产生首个 ≥ `PASTE_COALESCE_MIN_EVENTS` 的纯可重建键批,无模态,重建 ≥8 字符且逐事件匹配剪贴板头部
- **THEN** 立即以归一化原文 `insert_paste_fold`(label 行数 = 归一化原文 `split('\n')` 计数),不进入阻塞合批;`paste_tail` 置位、活动行显示「⋯ 接收粘贴」;该批不再入 `process_event_batch`

#### Scenario: 首批不足 8 字符经凑批后命中(top-up)

- **WHEN** 一次大粘贴的首个 `drain` 批仅含 4-15 个事件(重建 2-7 字符,像粘贴但不足匹配门),后续 chunk 以 ~8ms 间隔持续到达
- **THEN** 快路径分支以 grace 凑批(≤5 轮)将累积批凑至 ≥8 字符后尝试**一次**并命中:立即折叠 + 尾流丢弃,总判定延迟 ≤ 5×`PASTE_COALESCE_GRACE`;若凑批期间静默(真·短输入)→ 尝试不命中,累积批交桥接按既有路径处理、无重复消费

#### Scenario: 判定门槛表(纯函数可单测)

- **WHEN** 对 `try_fast_paste` 分别给:含 `PageUp` 批、重建 7 字符批、`read_clipboard` 返回 `Err`/全空白、IME 短句批(与剪贴板头部失配)、归一后 14 行且 499 字符的剪贴板(前缀命中但不达阈值)、命中 case
- **THEN** 依次 `None` / `None`(且 `read_clipboard` **零调用**,mock 计数断言)/ `None` / `None`(前缀快拒,不付全文扫描)/ `None` / `Some`(`fold_text` = 归一化原文、`tail` 游标已在批末)

#### Scenario: 不可靠字符跳过(emoji 内容零泄漏)

- **WHEN** 剪贴板为含国旗 emoji 的配置文本(如 `- name: '🇭🇰 GOMA-HK'`,astral 码点位于行中);流侧分别以「astral 事件被整个吞掉」与「astral 以合成 `Char` 投递」两种形态到达
- **THEN** 吞掉形态:后继字符(空格)到达时 matcher 跳过期望侧 🇭🇰 两码点、重试命中 → 全程 `Drop`、零转发零泄漏、不触发中止;投递形态:astral `Char` 直接匹配期望、不触发跳过;两形态折叠原文均含完整 emoji(保真)

#### Scenario: 换行 run 匹配对到达形态鲁棒

- **WHEN** 剪贴板为 CRLF 源多行文本(含空行);流侧分别以「每换行 1 个 `Enter`」与「每换行 2 个 `Enter`」两种形态投递前缀批与尾流
- **THEN** 两种形态下前缀匹配均命中、尾流均逐事件 `Drop`;折叠文本均为归一化原文(`\n` 换行,空行保真)

#### Scenario: 尾流转发——控制键结构性直通、打字失配转发

- **WHEN** `paste_tail` 活跃期,尾流批之间到达:`Esc`(agent 运行中)、`Ctrl+C`、用户敲的字符、鼠标滚轮批
- **THEN** `Esc`/`Ctrl+C` 为非可重建键 → **恒转发且不触碰 matcher 状态**(中断/退出/选区复制即时生效,结构性免吞——即便期望字符恰为 `c`,`Ctrl+C` 亦不参与内容匹配);用户字符与期望失配 → 转发入框;滚轮为非 key → 转发照常滚动;其后尾流恢复匹配 → 恢复丢弃(resync)

#### Scenario: 尾流期模态应答可达

- **WHEN** agent 运行中粘大段命中快路径,尾流期 agent 发出 `PermissionRequired`(模态经 ui_rx 照常弹出),用户按 `y`
- **THEN** `y` 与期望字符失配(常态)→ 转发 → 权限分支正常应答;若恰与期望字符相等被吞(单次),游标已推进,再按 `y` 失配转发应答成功(自愈,Non-Goal 声明)

#### Scenario: 尾流终止三层(吸收先于 done)

- **WHEN** 分别构造:尾流完整到达且源**不**以换行结尾(游标经字符匹配耗尽)、源**以换行结尾** × 双 `Enter` 形态(游标经尾部 run 越至末尾,紧邻尚有吸收残余 `Enter`,其后用户敲一个字符)、内容分歧(连续 16 次失配转发后洪流继续、夹 `Esc` 与鼠标事件)、尾流被截断后 key 静默 2s(期间仅鼠标事件)
- **THEN** 依次:游标达末尾即清态(批内余下事件转发,不依赖时钟);尾部 run 场景 MUST 保持吸收态吞掉残余 `Enter`(**不**转发、**不**成为 lone Enter 提交),用户字符到达才清态并转发该字符;streak 达 `PASTE_TAIL_ABORT_MISMATCHES` 进入护栏态——其后可重建键(洪流)恒丢弃不入框、`Esc` 与鼠标事件仍恒转发、`is_done()` 为 false,直至 2s 静默兜底清态;`PASTE_TAIL_QUIET_FALLBACK` 兜底清态(经事件批前检查或 spinner tick,提示不滞留)。清态后击键恢复正常入框

#### Scenario: 未命中回退慢路

- **WHEN** 粘贴样批未命中快路径(如剪贴板不达折叠阈值的小段粘贴)
- **THEN** 置一帧「⋯ 接收粘贴」提示后进入既有合批桥接与 `fold_candidate`/逐键路径,处理结果与无快路径时一致;额外代价仅一次剪贴板惰性读取与该帧提示

#### Scenario: 提示与 copy_hint 并存时 copy_hint 优先

- **WHEN** `paste_tail` 活跃期用户拖选并按选区复制键复制成功(`copy_hint` 置位)
- **THEN** 活动行右侧显示「已复制 N 字」(copy_hint 优先);其 TTL 过期后「⋯ 接收粘贴」恢复显示(若尾流仍活跃);复制行为本身不受尾流影响(拖选为鼠标事件转发,复制键为非可重建键、恒转发直通)

#### Scenario: 剪贴板内容不入日志、事件日志不失明

- **WHEN** `MYSTERIES_TUI_DEBUG_EVENTS=1` 下发生快路径命中的粘贴与尾流(含丢弃与转发)
- **THEN** 事件日志维持既有 redact 形态(`Char(<redacted>)`),不出现剪贴板文本片段;命中批与尾流丢弃事件各带 ` disposition=fast-paste` / ` disposition=tail-drop` 标记(run_tui 层记录);尾流转发事件仅由既有批处理日志记录一次(无标记),同一事件不双行

## MODIFIED Requirements

### Requirement: 粘贴突发合并输入(批量 drain 防误提交)

TUI 事件循环 SHALL 在每次有 crossterm 事件到达时,用**同步** `event::poll(Duration::ZERO)` + `event::read()` 把当前**已就绪**的事件抽干成一个有界 batch(不 await、不阻塞、与 `EventStream` 共用同一 internal reader 故不污染其 waker),整批处理完只渲染一次;并按"一批**文本内容键**(`Char` + 裸 `Enter`,Press-only)的规模"区分**粘贴突发**与**用户敲击**:突发(批内 ≥2 文本内容键)内的裸 `Enter` SHALL 作换行插入,仅当裸 `Enter` 是其批次里唯一文本内容键时才判提交。

**提交前续读(防跨批/跨周期粘贴误提交)**:因 ConPTY 会把一次大粘贴切成多个 batch 分次投递(`drain` 一次 `poll(ZERO)` 只抽干当前已就绪即停),某换行可能落在此刻 `n==1` 的独立批 → 被误判提交。故 `drain` SHALL 在抽干 `poll(ZERO)` 后,当当前 batch 经 `classify` 将得 `Submit`(落单裸 `Enter`,谓词 `would_submit_lone_enter(batch)` 为真)时,以 `poll(GRACE)`(默认 `PASTE_CONTINUATION_GRACE = 10ms`,具名常量)做一次续读:窗口内有事件 → 读入**同一 batch**,**若读入的是非键盘事件(鼠标 `Moved`/焦点/resize)SHALL 收批**(不无限等待,避免 `EnableMouseCapture` 的高频 `Moved` 令续读不退出、阻塞重绘与 agent 流式),否则回到抽干循环(粘贴续批全是 `Event::Key`,带来 `Char` 后 `n` 变大、谓词转 false、循环终止);窗口内静默 → 收批(真提交)。以 `EVENT_BATCH_CAP` 在**抽干与续读两条路径**封顶防无限续读。续读触发 SHALL 抽为纯函数 `would_submit_lone_enter(batch) = classify_key_batch(press_key_events(batch))` 含 `Submit`;续批读入后 SHALL **复用既有** `classify_key_batch` / `apply_batch_input_key`(裸 `Enter` 因 `n` 变大自然从 `Submit` 变 `Newline`),**不新增 intent 改写逻辑**。此信号取自 `drain` 内同步 `poll`、**不经 `draw` / `select!` / 墙钟**,故不被渲染时延、agent 流式事件、鼠标事件污染。

硬模态在批处理中**逐键按当时活跃态**分治(非整批截断):`pending_permission` 活跃时首键应答后丢弃该批余下键;`models_picker` 活跃时每键透传给 picker(打字过滤/导航/选中 MUST NOT 丢失)。「剪贴板校准粘贴快路径」requirement MAY 在抽干后、合批/续读**前**拦截大粘贴批(命中即折叠 + 尾流校准丢弃;未命中时仅额外一次剪贴板惰性读取与一帧接收提示——该帧为本 requirement「整批处理完只渲染一次」的显式例外——其余行为不变)。此机制 SHALL NOT 依赖 bracketed paste(Windows crossterm 不产 `Event::Paste`,已诊断探针证实)、SHALL NOT 改 `terminal.rs`、SHALL NOT 改 `select!` 事件循环结构,复用既有文本缓冲的 `InsertNewline`/`InsertStr` 动作与既有 `on_key` 路由。**已知上限(Non-Goal)**:①**大 transcript 慢渲染下正常打字凑批**(末字符 + 提交 `Enter` 落同批 → `n≥2` → `Enter` 误判换行、再按一次即提交;本 requirement 不碰 `classify` 的 `n≥2`→`Newline` 逻辑,该限制原样保留);②粘贴以换行结尾(续读窗口内无续批 → 末 `Enter` 仍提交、不自动换行;快路径命中时尾 `Enter` 属尾流被校准丢弃、不提交,见「剪贴板校准粘贴快路径」);③续批间隔慢到 > `GRACE` 的极端(跨秒/极慢分段粘贴)使落单换行仍被判提交;④粘贴含 Tab 丢失(快路径命中时 Tab 随剪贴板原文保真,见「剪贴板校准粘贴快路径」;慢路径维持此限);⑤模态关闭后同批粘贴尾 `Enter` 被丢弃——均需更强的到达建模或 bracketed paste(本栈不可用)。

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
- **THEN** 粘贴处理由 batch 突发启发式 + `drain` 内提交前续读 + 剪贴板校准快路径(命中时)实现,`Event::Paste` 分支维持忽略,`terminal.rs` 不变

### Requirement: 粘贴折叠占位符(大段粘贴折叠为原子 token)

TUI SHALL 在输入缓冲层把**大段粘贴**折叠为一个原子占位符 token:仅当输入来自粘贴(批级识别)且满足**逻辑行数 `≥ PASTE_FOLD_MIN_LINES`(=15)或字符数 `≥ PASTE_FOLD_MIN_CHARS`(=500)之一**时 MUST 折叠,手打多行与小段粘贴 MUST 照原样逐字符插入。占位符在输入框渲为一行 label,形态**按 chunk 的 `line_count` 分派、与触发原因无关**——多行 chunk(`line_count >= 2`,含行数不达而字符达标者)为 `[Pasted text #N +M lines]`,单行 chunk(`line_count == 1`)为 `[Pasted text #N +K chars]`(K = `chars().count()` 字符数,非字节数)——以 `text_muted` 弱化样式与正文区分;提交时 MUST 原位展开为完整文本。

**触发(批级重建)**:大粘贴命中「剪贴板校准粘贴快路径」时,折叠文本 SHALL 取剪贴板归一化原文、不经本段批级重建(仍经同一 `insert_paste_fold` 入口,存储/渲染/编辑/提交语义不变);其余情形如下。纯函数 `fold_candidate(batch, min_lines, min_chars) -> Option<String>` SHALL **仅当批的 press 键(`press_key_events`)逐键全为文本内容键**(`Char` 非纯 Ctrl + 裸 `Enter`,整批本质是一段纯粘贴)时,重建粘贴文本(`Char`→字符、裸 `Enter`→`\n`)并当 `split('\n').count() >= min_lines` **或** `chars().count() >= min_chars` 时返回 `Some(文本)`;只要出现任一非文本内容键(如 `PageUp`)或两阈值均不达,MUST 返回 `None`。`process_event_batch`(非 `run_tui` 主循环,其顶部持整批 `Vec<Event>`)SHALL 在无 `pending_permission`、无 `models_picker` 且 `fold_candidate` 命中时调 `insert_paste_fold`(`pub(crate)` 入口,封装私有 `apply_input_action`+`refresh_command_completion`)并消费整批,否则走既有逐键路径;paste-guard 跨批续读在上游 `drain_event_batch`,不受影响。

**存储(原子单字符 + 旁挂映射)**:`InputBufferState` SHALL 持 `pasted: BTreeMap<char, PastedChunk>`(`PastedChunk { seq, text, line_count }`)与 `next_paste_seq: u32`。`InsertPasteFold(s)` 动作 MUST 在 `cursor` 处插入一个私有区单字符 sentinel(`char::from_u32(0xE000 + seq)`)、记录 `pasted[sentinel] = { seq, text: s, line_count: s.split('\n').count() }`、`next_paste_seq += 1`。sentinel 为**单个 `char`**,故现有基于 char 边界的光标移动与 `Backspace`/`Delete` 逻辑 MUST NOT 改动即把占位符当**一个原子**(整体跨过、整体删除、光标不入内部)。

**编辑与清理(孤儿裁剪)**:占位符 SHALL 可与手打文字混排、可多个。`prune_pasted()` 的保留集 SHALL 为 **`text` ∪ `draft`** 中出现的 sentinel(历史召回时 chunk 可能仅被 `draft` 引用,text-only 裁剪会使 `↓` 还原后 sentinel 失配丢数据);`Backspace`/`Delete` 删除后、`SetText`(如命令补全)与 `history_up`/`history_down` 替换 `text` 后 MUST `prune_pasted()`。使 draft 还原路径永久不可达的动作 MUST 弃 draft(清空 `draft` 后裁剪),共三处:`exit_history`(召回态打字/编辑退出,仅原 `history_cursor.is_some()` 时生效,非召回态打字零额外开销)、`SetText`(无条件)、`history_down` **还原分支**(Some→None,消费即清空:`text = draft` 后置空 draft——不清则 stale draft 令删除后的 chunk 变 zombie、编号不归零)。裁剪后 `pasted` 为空时 `next_paste_seq` MUST 归零(无存活 chunk,重新编号安全)。显示编号 `#N = seq + 1`(seq 从 0):存活 chunk 期间单调、删除不回收;`pasted` 清空后从 `#1` 重计。

**提交(展开)**:提交时 prompt MUST 取 `expand_folds`(逐 char 把 sentinel 替回 `PastedChunk.text`)后的完整文本;命令旁路判据 MUST 计入 fold(`input().contains('\n') || input_has_fold()` 为真时不试 `parse_command`)。`PushSubmitted` MUST 以展开文本入 history,并清空 `pasted` + 置 `next_paste_seq = 0`(故 `↑` 召回显示展开文本、下一条 `#N` 从 `#1`(seq 0)起)。

**渲染(label dim,跨软换行分段)**:render 端 SHALL 以 `expand_for_display`(sentinel → label,**同时产出各 label 在 display 串中的字节区间**,区间有序、互不重叠、落在 char 边界)+ 光标偏移映射喂 `visual_input_layout`;`InputVisualLayout` SHALL 纯加法暴露 `line_starts`(各可视行在 display 串中的起始字节偏移),不变量:`lines.len() == line_starts.len()` 且 `display[line_starts[i] .. line_starts[i] + lines[i].len()] == lines[i]`。渲染 MUST 以可视行区间与 label 区间**相交**切分 span:label 段渲 `text_muted`、其余正文渲既有 `text_primary`,label 被软换行切开时每一段都 MUST dim;dim 判定 MUST 基于区间而非文本模式匹配(用户手打同款字面文本 MUST NOT 被 dim)。换行与 `input_content_height_cap` 按 label 宽度计入。空 `pasted` 时输入渲染 MUST 与折叠前一致(既有输入快照零 churn);dim 不改文本,既有折叠快照文本亦零 churn。

**Non-Goals(v1)**:不支持 `↑` 编辑/回折已提交的粘贴;不改 `apply_batch_input_key` 既有逐键路径;不保证 `↑↓` 垂直移动跨越/邻接 fold 时光标落位列与屏幕 label 宽度对齐(reduce 按 buffer 列 sentinel=1 算,label ~26 列,v1 接受、真机勿当回归);粘贴文本内**字面 PUA 字符**(U+E000..U+F8FF)与 sentinel 的理论撞车为接受边界(v1 即存在;seq 删空归零使复用窗口略扩,前提同为剪贴板含 PUA,一并接受),不处理。

#### Scenario: 大段粘贴(≥15 行)折叠为占位符 token

- **WHEN** 一批粘贴事件重建出 20 逻辑行文本,`process_event_batch` 时无模态
- **THEN** `fold_candidate` 返回 `Some(该文本)`;施 `InsertPasteFold` 后 `input_line.text` 在光标处含**一个** sentinel、`pasted` 有一项(`line_count=20`)、`next_paste_seq=1`;输入框该处渲为一行 `[Pasted text #1 +20 lines]`,不逐行撑满

#### Scenario: 单行超长粘贴按字符阈值折叠

- **WHEN** 一批粘贴事件重建出**无换行**的 600 字符单行文本(全文本内容键),`process_event_batch` 时无模态
- **THEN** `fold_candidate` 返回 `Some`(`行数 1 < 15` 但 `字符数 600 ≥ 500`);折叠后 `pasted` 一项(`line_count=1`);label 渲为 `[Pasted text #1 +600 chars]`;提交展开为原 600 字符

#### Scenario: 小段粘贴与手打多行不折叠

- **WHEN** 批重建出 14 逻辑行且总字符数 < 500(或手打的多行)
- **THEN** `fold_candidate` 返回 `None`;走既有逐键路径,文本逐字符/逐行进缓冲,`pasted` 为空、渲染同现状

#### Scenario: 折叠触发纯函数(可单测)

- **WHEN** 对 `fold_candidate(batch, 15, 500)` 分别给:14 个裸 `Enter`(+若干 `Char`,重建 15 逻辑行)的纯粘贴批、13 个裸 `Enter`(14 逻辑行、总字符 < 500)批、单行 600 字符批、单行**恰 500** 字符批、单行 499 字符批、14 行 × 40 字符(560)多行批、含 `PageUp` 的混批、空批
- **THEN** 依次:`Some`(行数达标)/ `None`(两阈值均不达)/ `Some`(字符达标)/ `Some`(`≥` 含边界)/ `None` / `Some`(行数不达、字符达标;折叠后 label 仍按 `line_count` 分派为 `+14 lines`)/ `None`(含非文本内容键)/ `None`;边界口径:N 逻辑行 = N−1 个裸 Enter(无尾随)、`count = 裸 Enter 数 + 1`

#### Scenario: 占位符为原子——方向键整体跨过、退格整体删除

- **WHEN** 缓冲为 `a⟦sentinel⟧b`(⟦⟧ 为一个折叠占位符),光标在末尾;先按 `MoveLeft` 两次,再于 sentinel 后按 `Backspace`
- **THEN** `MoveLeft` 一次跨过 `b`、再一次整体跨过占位符(光标落 `a` 后);`Backspace` 于 sentinel 后整体删除该占位符(不进入内部)、随后 `prune_pasted` 使 `pasted` 移除该项

#### Scenario: 删空后编号复位

- **WHEN** 缓冲仅含一个 fold(seq 0),`Backspace` 删除它后再粘贴一段可折叠文本
- **THEN** 删除后 `pasted` 为空且 `next_paste_seq == 0`;新 fold sentinel 复用 U+E000、label 显示 `#1`

#### Scenario: 历史召回往返保留 fold、退出召回弃 draft

- **WHEN** 缓冲含一个 fold,`↑` 召回历史条目后:一路 `↓` 还原(其后再 `Backspace` 删除该 sentinel);另一路直接打字
- **THEN** 还原路:`text` 复原含 sentinel,chunk 完好、label 正常渲染、提交可展开(`prune_pasted` 保留集含 `draft`,召回途中不杀 chunk),且还原**消费** draft(`draft` 为空);随后删除 sentinel → `pasted` 空、`next_paste_seq == 0`(无 stale draft 引用致 zombie);打字路:`exit_history` 清空 `draft`、其独占 chunk 被裁剪、`history_cursor == None`,`text` 中仍存在的 sentinel(若有)不受影响

#### Scenario: SetText 整体替换清孤儿

- **WHEN** `pasted` 持有 chunk(被 `text` 或 `draft` 引用)时发生 `SetText`(如命令补全整体替换输入)
- **THEN** `draft` 被清空、`prune_pasted` 以新 `text` 为准裁剪;新文本不含 sentinel 时 `pasted` 为空且 `next_paste_seq == 0`

#### Scenario: 提交展开为完整文本、history 存展开文本

- **WHEN** 缓冲为 `看这段:⟦#1(20 行原文)⟧`(单 sentinel、无字面 `\n`),按 Enter 提交
- **THEN** prompt = `看这段:` + 20 行原文(`expand_folds`);因含 fold **不**试 `parse_command`;transcript/history 收展开全文;提交后 `pasted` 空、`next_paste_seq=0`;`↑` 召回该条显示展开文本

#### Scenario: 混排多占位符按位置展开保序

- **WHEN** 缓冲为 `⟦#1⟧ 中间 ⟦#2⟧`,两占位符各自原文 A、B
- **THEN** `expand_folds` 得 `A 中间 B`(保序、各归各位);渲染为两个 label 按位置保序(各自形态按其 `line_count` 分派:多行 `+M lines`、单行 `+K chars`)

#### Scenario: line_starts 不变量(可单测)

- **WHEN** 对多逻辑行、软换行(宽度触发)、CJK 宽字符折行、空逻辑行、行恰满(cursor 行末溢出空行)诸 case 调 `visual_input_layout`
- **THEN** 均满足 `lines.len() == line_starts.len()` 且逐行 `text[line_starts[i] .. line_starts[i] + lines[i].len()] == lines[i]`(空行对空串平凡成立);既有 `lines`/`cursor` 断言零改动

#### Scenario: label dim 分段上色(跨软换行,带色断言)

- **WHEN** 输入为 `正文 + fold + 正文` 混排,视口宽使 label 软换行为两段;另有用户**手打**字面 `[Pasted text #1 +2 lines]` 的对照输入
- **THEN** label 两段所在 cell 的 fg 均为 `text_muted`,前后正文 cell 为 `text_primary`(按 `TestBackend` buffer cell 断言,主题无关按 token 比对);手打字面文本 cell 保持 `text_primary`(dim 判定基于 label 字节区间,非文本匹配)

#### Scenario: 折叠渲染与高度核算(insta 快照)

- **WHEN** 输入框含 `前缀文字 [Pasted text #1 +20 lines] 后缀文字`,`TestBackend` 渲染;另测窄宽两宽度与单行 `+K chars` label
- **THEN** 占位符渲为一行 label、与正文可辨;`visual_input_layout` 按 label 宽度换行、`input_content_height_cap` 不因 fold 偷 transcript 地板;空 `pasted` 布局同现状;既有快照零 churn,单行 label 快照新增锁定
