# Tasks — fix-paste-latency

红灯纪律:红灯独立成步,以断言失败落红(非编译错)——新签名/新类型允许红灯内先落桩(签名成型、空产出/旧语义)。每个红灯步内**显式区分**「断言红」(桩下必红,红灯证据)与「边界锁定」(桩下即绿,防实现走样,不作红灯依据)。**红灯停点**:2.1 / 3.1 为新接口首次成型,测试 + 失败输出贴出后**停下等主 agent 确认**再进绿灯;1.x / 4.x / 5.x 可连写。
执行 agent MUST NOT:git 写操作、修改既有快照/夹具以过测、勾选第 7 节真机任务。

## 1. clipboard:trait 扩 get_text(小面,可连写)

- [x] 1.1 `Clipboard::get_text(&mut self) -> Result<String, String>` 入 trait;`MockClipboard`(clipboard.rs tests,补可设文本/Err)与 `RecordingClipboard`(mod.rs tests)补实现(全仓 `impl Clipboard for` 恰三处);先写 mock 行为测试再实现(trait 追加方法,编译红豁免、仍先测后码);`ArboardClipboard::get_text` 经 `arboard`,`inner` 为 `None` → `Err("剪贴板不可用")`(与 `set_text` 同措辞)

## 2. input_batch:归一 / 重建 / matcher / 快路径判定(强制 TDD)

- [x] 2.0 重构(保绿):从 `fold_candidate` 抽共享 `rebuild_text(keys) -> Option<String>`(全为文本内容键才 `Some`;`fold_candidate` 语义零变,既有测试零改动)——先跑全量确认绿再进红灯
- [x] 2.1 红(**停点**):桩 = `normalize_newlines` 恒原样返回;`rebuild_text` 暂不认 `Tab`;`PasteTailMatcher::new(normalized)` + `on_event(event) -> TailAction { Drop, Forward }` + `is_done()` 桩恒 `Forward`/`false`;`try_fast_paste(batch, read_clipboard: impl FnOnce() -> Result<String, String>) -> Option<FastPaste>` 桩恒 `None`;常量 `PASTE_FAST_MIN_MATCH_CHARS = 8`、`PASTE_TAIL_ABORT_MISMATCHES = 16` 落位。测试:
  - 归一(断言红):`a\r\nb`→`a\nb`、孤 `\r`→`\n`、混合、尾 `\r\n`
  - 重建 Tab(断言红):含 `Tab` 键批 → `Some(含 \t)`;(边界锁定)含 `PageUp` → `None`
  - matcher 匹配丢弃(断言红):逐字符命中批全 `Drop` 且推进;`\n` run × 「每换行 1 个 `Enter`」/「每换行 2 个 `Enter`」两形态均 `Drop`(吸收态);空行 run(`a\n\nb`)两形态;`Tab` 匹配 `\t`
  - matcher 失配与分类(「不推进/resync 恢复 `Drop`/Release 恒 `Drop`」为断言红——`Forward` 侧对恒-`Forward` 桩即绿,标**边界锁定**):期望字符处给非期望 `Char` → `Forward` 且不推进(经后续 `Drop` 观察);转发后尾流恢复匹配 → 恢复 `Drop`(resync);**期望字符恰为 `c` 时给 `Ctrl+C`** → `Forward` 且不推进不触状态(非可重建键不参与匹配,唯一有区分力的构造);`Esc`/方向键 → `Forward` 不触状态;非 key 事件 → `Forward` 不退出吸收态(吸收态中夹鼠标事件,后续 `Enter` 仍被吸收);Release 恒 `Drop` 不计状态
  - matcher 终止(断言红):后缀耗尽(源不以换行结尾)→ `is_done()`、其后事件恒 `Forward`;**源以换行结尾 × 双 `Enter` 形态:游标经尾部 run 越至末尾后紧邻残余 `Enter` 仍 `Drop`(吸收先于 done,不得转发成 lone Enter)**,用户字符到达才 `is_done()` 并 `Forward` 该字符;连续 16 次失配转发 → 中止(`is_done()`,其后恒 `Forward`);匹配一次后 streak 归零(15 失配 + 1 匹配 + 15 失配不中止)
  - matcher 移交(断言红):try_fast_paste 命中批以 `Enter` 结尾(行中换行恰落批末)× 双形态 → 移交的 matcher 处于吸收态,尾流首个残余 `Enter` 即 `Drop`
  - `try_fast_paste` 门槛(命中/CRLF 两形态命中为断言红;各拒绝门为边界锁定):批 < `PASTE_COALESCE_MIN_EVENTS` 由调用方前置(不在函数内测);含非重建键 → `None`;重建 7 字符 → `None` 且 **`read_clipboard` 零调用**(mock 计数断言);剪贴板 `Err`/全空白 → `None`;前缀失配 → `None`(IME 短句 vs 无关剪贴板);归一后 14 行且 499 字符 → `None`(不达阈值,前缀虽命中);命中 → `Some`,`fold_text` = 归一化原文(尾随换行 `split` 口径 +1)、`tail` 已步进到批末位置(紧接尾流首字符即 `Drop`)
- [x] 2.2 绿:实现四件;**重建分两口径**(停点修正:共享 `rebuild_text` 若认 Tab 会连带改变 `fold_candidate` 语义,违反 MODIFIED「粘贴折叠占位符」的「全为文本内容键」契约)——`rebuild_text`(文本内容键,`fold_candidate` 专用,语义零变)与 `rebuild_fast_text`(可重建键,含 Tab,快路径专用),私有共享核 + 两薄壳;2.1 的 Tab 红灯测试改打 `rebuild_fast_text`;补边界测试:`fold_candidate` 含 `Tab` 的达阈值批 → `None`(锁分口径);补空行 run **单 Enter 形态**一条断言(2.1 只测了双形态)。`try_fast_paste` 门序 = 可重建键 → ≥8 字符 → 读剪贴板+归一 → **前缀匹配(matcher,快拒)** → 阈值;命中把 matcher 移交返回值

## 3. app:paste_tail 瞬态(强制 TDD)

- [x] 3.1 红(**停点**):桩 = `AppState.paste_tail: Option<PasteTailState>` 字段 + `set_paste_tail` / `paste_tail_active()` / `clear_paste_tail` 恒空实现;`PASTE_TAIL_QUIET_FALLBACK = 2s` 落位。测试:
  - 置态/清态/活跃查询(断言红)
  - 2s 兜底:仅**匹配丢弃**的 key 刷新 `last_key_at`(转发不刷新);注入 now,`now+2s` 过期(断言红;仿 copy_hint 的注入 now 模式——注意 copy_hint 先例只有惰性过滤,主动清态为新增行为,测试直接锁 `expire_paste_tail(now)` 纯函数面)
- [x] 3.2 绿:最小实现

## 4. render:提示(事后回归,小面可连写)

- [x] 4.1 活动行右对齐「⋯ 接收粘贴」(`text_muted`,复用 copy_hint 渲染通路);**copy_hint 优先**(同时活跃显 copy_hint,4s TTL 过期后 paste 提示恢复)——spans 级断言 + 新快照一张;既有快照零 churn(此为渲染接线,不设红灯,断言先写后接通亦可)

## 5. mod.rs:drain 拆分与接线(IO 胶水,事后回归)

- [x] 5.1 `drain_event_batch` 拆 `drain_immediate`(`poll(ZERO)` 抽干 + CAP,满批不进后续判定)与 `bridge_event_batch`(合批 30ms / lone-enter 10ms / grace 每轮重评 / 读入回抽干 / 非 Key 收批 / 桥接路 CAP)——语义逐条对照现函数,除拆分外一行不改;全量测试零回归
- [x] 5.2 run_tui 事件臂:`drain_immediate` → **批前 2s 兜底过期检查** → `paste_tail` 活跃则逐事件过 matcher(`Drop` 弃 + 刷新兜底计时、`Forward` 集合经 `process_event_batch` 照常处理、`is_done` 清态),完 → 否则批像粘贴且无模态时 `try_fast_paste`(惰性闭包内 `clipboard.get_text`):命中 → `insert_paste_fold(fold_text)` + `set_paste_tail(matcher)`(**matcher 已 done 则不置态**,整段落首批时提示不空挂)+ 立即重绘,不入桥接不入 process → 未命中 → 置提示、画一帧、`bridge_event_batch` 照旧 → 其余批直接 `bridge_event_batch`。tick 臂:2s 兜底清 `paste_tail`(与批前检查共两处判定点,spec MUST)
- [x] 5.3 事件日志:快路径命中批与尾流**丢弃**事件由 run_tui 层写 `MYSTERIES_TUI_DEBUG_EVENTS` 日志(既有 redact 不变),行尾 ` disposition=fast-paste` / ` disposition=tail-drop`;尾流**转发**事件不在 run_tui 层记录(由既有 `process_event_batch` 顶部日志记录一次,无标记,避免双行);不新增任何含剪贴板内容的日志
- [x] 5.4 **预试凑批(真机 1080 行 6s 修订)**:快路径分支中,批像粘贴但 `rebuild_fast_text` 不足 8 字符时,以 `PASTE_COALESCE_GRACE` 最多 `PASTE_FAST_TOPUP_ROUNDS`(=5,新常量)轮 grace 凑批(读入回抽干、非 key 收批,与 `bridge_event_batch` 同规,可抽共享助手),凑满 ≥8 字符或静默即止,再对累积批恰一次 `try_fast_paste`;未命中 → 累积批交 `bridge_event_batch` 续走(无重复消费、无事件丢失)。同时:尝试未命中时 debug 模式记 `paste-fast decline reason=<too-short|no-match|clipboard-err|below-threshold> rebuilt_chars=N batch_len=N`(不含内容;`try_fast_paste` 签名如需改为带原因枚举,纯函数面补对应单测——原因分派为断言红,先红后绿)

- [x] 5.5 **中止护栏 + 日志缓冲(真机二次修订:内容分歧致 16 连失配 → 清态后 11.9 万事件涌普通路 + 逐事件文件 I/O 卡死)**:
  - matcher(纯逻辑,红绿):abort(streak ≥16)改为进入 aborted 护栏态——可重建键恒 `Drop`、非可重建键/非 key 恒 `Forward`、`is_done()` 保持 false(由 2s 静默兜底收场)。红灯:改造既有 `matcher_aborts_after_sixteen_consecutive_mismatches`(现断言 abort 后 `is_done()` 且匹配字符也 `Forward`——按新契约反转:abort 后 `is_done()==false`、可重建键(含恰匹配字符)`Drop`、`Esc`/鼠标 `Forward`),先红后绿
  - abort 观测:debug 模式记 `paste-tail abort streak=16 cursor=N normalized_len=M`(matcher 暴露只读 `cursor()`/`normalized_len()` 或 abort 信息,接线在 run_tui 分流处)
  - `append_debug_event_line` 改 `OnceLock<Mutex<BufWriter<File>>>` 持久句柄 + 批边界 flush(行为面零变:内容/redact/格式/路径不动;12 万事件级尾流的逐行 open-close 是卡死放大器)
- [x] 5.6 尾流纯丢弃批跳过即时重绘(tick 兜底)
- [x] 5.7 **不可靠字符跳过(真机三次修订:Clash 配置国旗 emoji,ConPTY 吞 astral 致 cursor=35 恒中止)**(纯逻辑,红绿):
  - matcher:失配且期望为不可靠字符(`> U+FFFF` / `U+FE0F` / `U+200D`)→ 跳过期望侧连续不可靠字符,对同一事件重试一次;重试命中 → `Drop` + 推进 + streak 归零;仍失配 → 既有失配转发路。红灯(断言红):`'🇭🇰 GOMA'` 内容 × 流侧无 astral 事件 → 全 `Drop` 且推进到底(现状:空格处 `Forward`);`🇭🇰` 后接换行 run × 双 `Enter` 形态;emoji 位于内容末尾(跳过后 done 语义);`U+FE0F`/`U+200D` 同规;(边界锁定)流侧真投递 astral `Char` → 直接匹配不触发跳过;连续多个 emoji(如 `'🇭🇰🇺🇸 x'`)一次跳过;跳过后重试仍失配 → `Forward` 且 streak 计数
  - 前缀匹配阶段自动继承(同一 matcher);补一条 `try_fast_paste` case:首行含 emoji 的剪贴板 × 无 astral 事件流 → 命中且 `fold_text` 含完整 emoji

## 6. 门禁

- [x] 6.1 `cargo test --lib` 全绿;`cargo clippy --all-targets -- -D warnings` 零警告;快照仅预期新增、既有零 churn
- [x] 6.2 `openspec validate fix-paste-latency --strict` 通过

## 7. 真机核验(主 agent / 用户;执行 agent MUST NOT 勾)

- [x] 7.1 **提速主证**:粘 600+ 行大段 → label 体感即时(<0.5s,对照修前 ~5s;**首轮真机 1080 行 6s 未过——快路径未咬合,凑批修订后复测**);label 行数与源行数一致(`split` 口径,源以换行结尾则 +1;CRLF 源验证归一);若仍慢,开 `MYSTERIES_TUI_DEBUG_EVENTS=1` 看 `paste-fast decline reason=` 行定位
- [ ] 7.2 尾流:大粘贴后数秒内活动行显示「⋯ 接收粘贴」;期间**打字直接上屏**(失配转发,不再被吞);期间 `Esc`(agent 运行中)即时中断、滚轮滚动正常;提示随尾流耗尽消失
- [ ] 7.3 模态:agent 运行中粘大段 → 尾流期弹出权限框 → 按 `y` 正常应答(偶发单次吞键属期望字符撞车,再按即自愈)
- [ ] 7.4 回退:粘 5 行小段 → 慢路照常(逐字入框/不折叠);粘 20 行 → 快路即时折叠;粘贴前先把剪贴板换成无关内容再高速敲 8+ 字符(模拟失配)不误折
- [ ] 7.5 既有行为零回归:多行粘贴不逐行提交;lone Enter 正常提交;`/` 补全、权限框、models picker 各模态下粘贴同现状;尾流未尽时二次粘贴 → 以普通输入涌入(已知边界,现象同今日粘贴中再粘),尾流耗尽后二次粘贴照常快路

> **收尾记录(2026-07-04,主 agent 依真机证据定案)**:7.1 核验通过——截图证据(1085 行即时折叠、零字符泄漏、「⋯ 接收粘贴」提示正常)+ debug 日志取证(`disposition=fast-paste` 76 条、`tail-drop` 24 万条、`paste-tail abort` **零条**,emoji 跳过闭环)。7.4 之「失配不误折」由日志尾部自然发生的 `paste-fast decline reason=no-match`(回退慢路、无误折)佐证。7.2 之提示显示/尾流丢弃有截图与日志证据,其 `Esc`/滚轮子项及 7.3(模态尾流)、7.5(逐项回归)未逐项跑通,依四轮真机迭代实际覆盖 + 585 项单测,评估为低风险入观察池。**已知限制(用户拍板暂接受)**:尾流接收期(数秒)用户 Enter 与内容换行在事件层不可区分,撞车即被吞,发送不可靠——需待提示消失后回车;定案与备选见决策记录 log 47。
