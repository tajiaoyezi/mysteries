# Design — fix-paste-latency

## Context

现状路径:事件到达 → `drain_event_batch`(`poll(ZERO)` 抽干 + 批 ≥ `PASTE_COALESCE_MIN_EVENTS`(=4) 时以 `PASTE_COALESCE_GRACE`(=30ms) 桥接后续 chunk,阻塞至整段收齐)→ `process_event_batch` → `fold_candidate` 折叠。阻塞时长 = ConPTY 投递整段的时间(实测 610 行 ≈ 5s),期间不重绘。合批桥接本身**未入 spec**(实现细节,log 40),重构自由度在;spec 契约面在「粘贴突发合并输入」的抽干 + 10ms 续读语义,须逐条保持。

本设计经一轮三路对抗审查后重订:初稿的「尾流按 key 静默 150ms 定时丢弃」被坐实两处 HIGH——① 吞模态应答/Esc/Ctrl+C 且被人类按键自续命;② 静默期与事件循环慢帧竞速,过期后残流入框成乱码/垃圾二次 fold。定案改为**内容校准丢弃**(见 D2),两族问题结构性消解。

## D1 快路径判定:惰性读取 + 门槛链 + run 感知前缀匹配

`try_fast_paste(batch, read_clipboard) -> Option<FastPaste>`——`read_clipboard: FnOnce() -> Result<String, String>` **惰性注入**(纯函数面 + mock 可数调用次数,「不读剪贴板」可观察);`FastPaste { fold_text: String, tail: PasteTailMatcher }`。门槛按序短路,前廉后贵:

1. 调用方前置:无 `pending_permission`、无 `models_picker`(与 `fold_candidate` 同门)、`batch.len() >= PASTE_COALESCE_MIN_EVENTS`;
2. 批内 press 键全为**可重建键**(`Char` 非纯 Ctrl、裸 `Enter`、`Tab`;重建映射 `Char`→字符、`Enter`→`\n`、`Tab`→`\t`;比 `fold_candidate` 多认 Tab——快路径命中时 Tab 随剪贴板原文保真,部分改善 log 38 Non-Goal ④);
3. 重建文本 `chars().count() >= PASTE_FAST_MIN_MATCH_CHARS`(=8):此门在读剪贴板**之前**——短打字/IME 短句突发不触发剪贴板读取;
3½. **预试凑批(top-up,真机修订)**:批像粘贴(≥ `PASTE_COALESCE_MIN_EVENTS`)但重建不足 8 字符时,MUST 在快路径分支内以 `PASTE_COALESCE_GRACE` 做最多 `PASTE_FAST_TOPUP_ROUNDS`(=5)轮 grace 凑批(读入后回抽干、非 key 收批,与桥接同规),凑满 ≥8 字符或静默/收批即止,然后对累积批**恰一次**尝试;仍不足或未命中 → 累积批交桥接续走慢路(无重复消费)。**为什么**:初稿把「首批不足 8 字符」当低频 Non-Goal,真机 1080 行实测 6s 坐实其为常态——select 在首个事件到达即唤醒,`drain_immediate` 此刻常只见开头几条记录,快路径一次性判定全程未咬合,整段被桥接吞走。top-up 上界 5×30ms=150ms,对照 6s 慢路可忽略;打字突发(<4 事件)不进快路径分支,零打字延迟代价;
4. **此时才调 `read_clipboard()`**;`Err`/全空白 → 拒;换行归一(`\r\n`→`\n`、孤 `\r`→`\n`);
5. **前缀匹配(先于阈值全扫,失配快拒)**:以 `PasteTailMatcher`(见 D2,换行 run 感知)从归一化剪贴板头部逐事件推进 batch 的 press 键——**全部匹配(零转发)**才算前缀命中;任一失配 → 拒。IME/打字突发的文本几乎必然 ≠ 剪贴板头部,在此廉价折返,不付全文扫描;
6. 归一化剪贴板满足折叠阈值(`行数 >= PASTE_FOLD_MIN_LINES || 字符数 >= PASTE_FOLD_MIN_CHARS`,与慢路同常量)→ 否则拒(小段粘贴本就不慢)。

命中:`fold_text` = 归一化原文,经既有 `insert_paste_fold` 入口插入,行数/字符数以归一化原文为真值(行数为 `split('\n')` 口径,尾随换行计入 +1,与折叠 requirement 既有口径一致);`tail` = 步进到当前匹配位置的 matcher,交尾流丢弃(D2)。

**为什么 run 感知匹配而非逐字符**:流侧换行到达形态未证实(CRLF 可能投 1 或 2 个 `Enter`),逐字符比对在换行处脆断。matcher 把「归一化剪贴板中的极大 `\n` run」与「事件流中的连续 `Enter` run」作为匹配单元(首个 `Enter` 越过整个 `\n` run 并进入吸收态,后续紧邻 `Enter` 吸收不推进,非 `Enter` 事件退出吸收态),对 CRLF/CR/LF 及可能的双投递一律鲁棒。匹配仅为同一性检验 + 定位,插入内容为剪贴板原文,空行/Tab 保真不受影响。

**弃**:
- eager 传 `clipboard_text` 参数(初稿;门 3「读之前」不可实施不可观察,审查坐实);
- 阈值全扫先于前缀匹配(初稿;IME 突发常态放行到门 4,10MB 剪贴板下每句白付全文扫描);
- collapse 折叠比对(初稿;可行但与尾流需要的 matcher 重复造轮,一个 run 感知状态机两处复用更小);
- 在剪贴板中搜索偏移以容忍「首 chunk 1 字符漏插」(复杂度不值 v1;漏插场景前缀失配退慢路,不劣化)。

## D2 尾流丢弃:内容校准(定案),弃定时丢弃(初稿)

**`PasteTailMatcher`(纯状态机,TDD 核心)**:持 `normalized: String, cursor: usize, in_newline_run: bool, forwarded_streak: usize`。事件分四类:**非 key 事件**(鼠标滚轮/Moved/焦点/resize)、**key Release**、**可重建键 press**(`Char` 非纯 Ctrl、裸 `Enter`、`Tab`,与 D1 门②同口径)、**非可重建键 press**(纯 Ctrl 组合如 `Ctrl+C`、`Esc`、方向/功能键等)。规则:

- **非 key 事件 → 转发,且 MUST NOT 触碰 matcher 状态**(不退出吸收态、不计 streak;滚动、拖选照常);
- **key Release → 丢弃**(内容无关,下游本就经 `is_key_press` 忽略;不触碰状态);
- **非可重建键 press → 恒转发,且 MUST NOT 触碰 matcher 状态**——`Esc`/`Ctrl+C`(退出/中断/选区复制)等控制键**结构性免吞**(内容匹配对它们无定义,连「撞车」都不可能),吸收态不因其退出(其后的吸收残余 `Enter` 仍被吸收);
- **可重建键 press 按期望推进**:
  - 期望为 `\n`(`cursor` 落在 `\n` run 起点)且事件为裸 `Enter` → 越过整个 run、置 `in_newline_run`、丢弃、`streak = 0`;
  - `in_newline_run` 且事件为裸 `Enter` → 吸收(丢弃、不推进,容双投递);**可重建的非 `Enter` 键**到达才退出吸收态、再按期望匹配;
  - 期望为字符 `c` 且事件重建为 `c`(`Char`→字符、`Tab`→`\t`)→ 丢弃、推进、`streak = 0`;
  - **不可靠字符跳过(真机三次修订)**:失配且期望为「流侧不可靠字符」(astral 码点 `> U+FFFF`、`U+FE0F` VS16、`U+200D` ZWJ)时,跳过期望侧连续不可靠字符后对**同一事件**重试匹配一次——真机 1085 行 Clash 配置坐实 **ConPTY 整个吞掉 astral 字符**(泄漏事件计数反推:🇭🇰 两码点零事件投递),matcher 在 `cursor=35` 等一个永不到达的 🇭 → 16 连失配全程护栏。重试命中 → 丢弃推进(零泄漏);仍失配 → 走失配转发。终端真投递合成 astral `Char` 时正常匹配分支先命中、不触发跳过,两种投递形态都成立。折叠原文不受影响(emoji 保真入 fold;对照:慢路事件重建从来就静默丢 emoji,快路径首次保住);
  - **失配 → 转发该事件**(粘贴后打字/模态应答直通既有处理),`streak += 1`;后续事件继续对同一 `cursor` 重试(resync:尾流恢复匹配即恢复丢弃);
- **终止**(三层,**吸收先于 done**):
  1. **精确**:`cursor` 达 `normalized` 末尾**且不在吸收态** → 清态,批内余下事件转发(不依赖任何时钟,慢帧免疫)。`cursor` 经**尾部 `\n` run** 越至末尾时 MUST 保持吸收态继续吞紧邻 `Enter`(源以换行结尾 × 双 `Enter` 形态的吸收残余;若 done 先于吸收,残余 `Enter` 被转发成 lone Enter → **误提交**——二轮审查 B-1 坐实),直到可重建非 `Enter` 键到达(清态并转发该键)或兜底超时;
  2. **中止 → 护栏模式(真机二次修订)**:`forwarded_streak >= PASTE_TAIL_ABORT_MISMATCHES`(=16)→ 内容跟踪失效,matcher 进入 **aborted 护栏态**:可重建键 → 恒 `Drop`(继续吞洪流,不再内容比对)、非可重建键与非 key → 恒 `Forward`(`Esc`/`Ctrl+C`/滚轮直通不变),`is_done()` 保持 false,由既有 2s 静默兜底(`Drop` 刷新计时)收场 | **弃**:中止即清态交还普通路径(初版;真机 1080 行坐实内容分歧真实存在——尾流 ~24 字符处 matcher 整体错位、16 连失配,清态后 11.9 万事件涌向普通路:漏内容入框 + 下一 flood 批 no-match 落阻塞桥接 + debug 日志逐事件 I/O = 分钟级卡死。fail-open 不可接受,护栏态 fail-safe:代价 = 洪流结束后 ≤2s 可重建键被吞,有提示可见)。中止时 debug 模式 SHALL 记一行 `paste-tail abort streak=16 cursor=N normalized_len=M`(定位脱轨内容位置,不含内容);
  3. **兜底**:key 事件静默 `>= PASTE_TAIL_QUIET_FALLBACK`(=2s,具名常量;仅匹配丢弃的 key 刷新计时)→ 清态(尾流被截断/永不完整/尾部吸收态悬置时提示不滞留);判定点 = **事件批处理前 + 既有 120ms spinner tick 臂两处**(tick 主动清态为**新增行为**——copy_hint 先例只覆盖「持 `Instant`/注入 now 可测/tick 驱动重绘」,其过期是渲染侧惰性过滤,不含主动清;如实区分)。

**这套机制消解初稿两 HIGH**:① 人类按键靠失配转发直通,不再被吞、不再自续命——控制键(非可重建)结构性免疫;模态应答键(可重建 `Char`)恰与期望字符撞车时该次被吞,游标经等字符链推进、泄漏字符恒等于用户所敲(晚一事件上屏,视觉自洽),经一次转发自动重对齐、不链式中止(二轮审查 B-10 推演);② 终止主路径是内容耗尽而非墙钟,事件循环慢帧、agent 流式重绘、滚轮全量重算都不再构成竞速面;2s 兜底仅在尾流真断流时起效,WT 片间隔 ~8ms,裕度 250×。

**弃**:
- key 静默 150ms 定时丢弃(初稿,两 HIGH 见 Context);
- 事件计数预测尾流长度(CRLF 形态未知,计数不可靠;内容校准天然免疫);
- 控制键白名单直通(失配转发已泛化覆盖,无需枚举键表);
- 丢弃期批内首个失配后整批转发(单个人类键会把同批粘贴片段一起放进输入框;逐事件 resync 无此漏)。

**如实边界(入 spec Non-Goal)**:模态应答键(可重建的 `Char`)与期望字符撞车的单次吞键(自愈:游标经等字符链推进,泄漏字符恒等于用户所敲,视觉自洽;控制键因非可重建而结构性免疫);二次粘贴落在首段尾流未耗尽前 → 其事件对 matcher 大概率失配转发、以普通输入涌入(现象与今日粘贴中再粘相同,不新增劣化;尾流耗尽后的二次粘贴照常走快/慢路);中止(streak 16)后的残余尾流按普通输入进入;吸收态被批间隙的**用户可重建键**打断(该键落在 CRLF 双 `Enter` 对的 ~8ms 间隙且恰在 chunk 边界)时,残余 `Enter` 失配转发、可能成为换行或 lone Enter 提交——双 `Enter` 形态未证实 × 间隙 × 边界三重小概率,接受并由 disposition 日志可观测。

## D3 提示:复用 copy_hint 渲染位,copy_hint 优先

「⋯ 接收粘贴」(`text_muted`)显示于活动行右侧(与「已复制 N 字」同位同款式):

- **快路径**:`paste_tail` 活跃期显示,清态即消;**与 copy_hint 同时活跃时 copy_hint 优先**(动作反馈优先于状态提示;copy_hint 4s TTL 过期后 paste 提示恢复)——初稿的 paste 优先会构成对「复制成功轻提示」requirement 的无声行为改写(其唯一让位条款是宽度不足),反转后该 requirement 零触碰;
- **慢路径**:批像粘贴(≥ `PASTE_COALESCE_MIN_EVENTS`)但快路径未命中 → 进阻塞合批**前**渲染一帧带提示画面(阻塞期用户可见状态),批处理完随后续帧消失。此帧是「整批处理完只渲染一次」的**显式例外**,在 MODIFIED 拦截句中声明。

## D4 drain 拆分与接线

`drain_event_batch` 拆两段,语义逐条保持:
- `drain_immediate(ev0)`:仅 `poll(ZERO)` 抽干(含 `EVENT_BATCH_CAP` 封顶)——即现函数内层循环;CAP 满批与现状同:不进桥接(新流程中亦不进快路径判定,直接交批处理,维持理论路径零新增);
- `bridge_event_batch(batch)`:现函数其余——粘贴合批 `PASTE_COALESCE_GRACE` 桥接 + lone-enter `PASTE_CONTINUATION_GRACE` 续读、grace 每轮按批规模重评、桥接一次一读、读入后**回到抽干循环**、非 Key 即收批、桥接路径 `EVENT_BATCH_CAP` 封顶;
- run_tui 事件臂顺序:`drain_immediate` → **批前兜底过期检查** → 若 `paste_tail` 活跃:逐事件过 matcher(丢弃/转发分流,转发集经 `process_event_batch` 照常处理,`is_done` 即清态),完;→ 否则若批像粘贴且无模态:`try_fast_paste`(惰性读剪贴板):命中 → `insert_paste_fold(fold_text)` + 置 `paste_tail`(**matcher 已 done 则不置态**——整段落入首批时无尾流,提示不空挂)+ 立即重绘,**不入桥接、不入 `process_event_batch`**(不重复插入;批内非 key 事件随批吞掉,与今日慢路 fold 命中即 return 同构,非新增劣化);未命中 → 置提示、画一帧、`bridge_event_batch` 照旧;→ 其余批直接 `bridge_event_batch`(内部门槛自然短路,行为同现状)。快路径命中帧与首个尾流批之间无双插窗口:置态在同一事件臂内同步完成,后续批必见活跃态;
- spinner tick 臂:2s 兜底过期清 `paste_tail`(与批前检查共两处判定点);
- `select!` 各臂结构不变(不增臂、不增 await 点)、`terminal.rs` 不变、`Event::Paste` 分支维持忽略。

**弃**:丢弃态放 run_tui 局部变量(提示要进 `render`,`AppState` 持瞬态有 copy_hint 先例);drain 内嵌快路径(drain 无 state/clipboard 访问,保持纯 IO)。

## D5 常量

`PASTE_FAST_MIN_MATCH_CHARS = 8`、`PASTE_TAIL_ABORT_MISMATCHES = 16`、`PASTE_TAIL_QUIET_FALLBACK = 2s`、`PASTE_FAST_TOPUP_ROUNDS = 5`(具名,置 mod.rs 既有粘贴常量组);阈值复用 `PASTE_FOLD_MIN_LINES` / `PASTE_FOLD_MIN_CHARS`、突发门槛复用 `PASTE_COALESCE_MIN_EVENTS`。

## D6 观测与隐私

- 剪贴板读取仅在粘贴样突发判定内发生(用户刚执行粘贴 = 语义授权);剪贴板内容 MUST NOT 进入任何日志;
- **事件日志不失明**:快路径命中批与尾流**丢弃**事件仍写 `MYSTERIES_TUI_DEBUG_EVENTS` 事件日志(既有 redact 形态,`Char(<redacted>)` 格式照旧),行尾分别加 ` disposition=fast-paste` / ` disposition=tail-drop` 标记(这两类不经 `process_event_batch`,不记则整体失明——初稿如此,恰好瞎掉本 change 新风险面的唯一真机观测手段);尾流**转发**事件不在 run_tui 层记录,由既有 `process_event_batch` 顶部日志自然记录(无标记;夹在 `tail-drop` 行之间可辨识),避免同一事件双行污染信噪;
- **decline 观测(真机修订)**:快路径尝试后未命中时,debug 模式 SHALL 记一行 `paste-fast decline reason=<too-short|no-match|clipboard-err|below-threshold> rebuilt_chars=N batch_len=N`(不含任何内容字符)——1080 行 6s 事故的定位靠猜,补上后一行日志即见卡在哪个门;
- **日志写入改持久缓冲(真机二次修订)**:`append_debug_event_line` 现状每行「open-写-close」,12 万事件的尾流 = 分钟级文件 I/O,是卡死事故的放大器(log 40 记过该反压,本 change 使其成承重面)——改 `OnceLock<Mutex<BufWriter<File>>>` 持久句柄,批处理边界 flush;行为面(内容/redact/格式)零变;
- 假阳性(≥8 字符突发恰为剪贴板头部,如 IME 整句提交撞上自己刚复制的草稿开头)的**主代价是剪贴板全文被误折入框**(fold 原子、单次 `Backspace` 可删)+ 尾流丢弃期人类失配键照常转发;概率极低 + 恢复成本一键,接受并如实入 Non-Goal。

## D7 性能口径

- 命中路(610 行 ≈ 30KB):`get_text` ~ms + 归一/matcher 亚 ms + 单 sentinel 插入 O(1) + 一帧渲染 → 体感 <0.1s;10MB 级剪贴板命中一次性 ~0.1-0.3s(仍远优于事件流的分钟级)。内存口径如实:命中后 `PastedChunk.text` 与 matcher 的 `normalized` 各持一份归一化全文,双份驻留至尾流清态(数秒),之后仅 chunk 一份;
- 未命中路:门 1-3 纯批内计算;门 4 起才付 `get_text`(一次 OS 拷贝,10MB 级 ~几十 ms,IME 长句 + 巨剪贴板并存的场景罕见,接受)、门 5 前缀匹配 O(批长) 快拒——全文扫描只在前缀已命中后发生;
- 尾流期每批全量重绘为新增 CPU 形态(现状同窗口零重绘),换 UI 存活;内容校准不依赖时钟,慢帧只慢不破。

## D8 测试与验收

- **强制 TDD(纯逻辑)**:换行归一(CRLF/CR/孤 CR/混合/尾换行);重建**分两口径**——`rebuild_text`(文本内容键,`fold_candidate` 专用、语义零变)与 `rebuild_fast_text`(可重建键,含 Tab,快路径专用),私有共享核 + 两薄壳(2.1 停点修正:单一共享函数认 Tab 会连带改 `fold_candidate` 语义、违反折叠 requirement 契约);`PasteTailMatcher` 全谱(逐字符匹配丢弃/`\n` run × 单双 `Enter` 两形态/吸收态退出/失配转发 + resync/连续失配中止/耗尽精确清态/Release 丢弃/非 key 转发/`Esc`/`Ctrl+C` 失配直通/模态应答撞字符的单吞与自愈);`try_fast_paste` 门槛表(逐门一拒 + 「门 3 拒时 `read_clipboard` 零调用」以 mock 计数断言 + 命中形态含 CRLF 单双 `Enter`);
- **Clipboard trait**:`get_text` 入 trait,`MockClipboard` / `RecordingClipboard` 两处 mock 补实现(全仓 `impl Clipboard for` 恰三处,已核);
- **接线(IO 胶水,不适用红绿)**:drain 拆分以既有全部纯测零回归为门禁;真机清单验收;
- **快照**:活动行「⋯ 接收粘贴」一张;copy_hint 优先级为 spans 级断言;既有快照零 churn。
