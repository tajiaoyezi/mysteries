# Design — guard-paste-burst-submit

## D0 —— 背景约束(已核实,勿推翻)
- 主循环(mod.rs L127-212):`tokio::select!` 三臂 `events.next()`(crossterm `EventStream`)/ `ui_rx.recv()` / `spinner_tick`(**无条件 120ms interval,idle 也 fire**);**每轮 select 后 `terminal.draw()` 一次**(L204),即每个键事件后都全量渲染(O(transcript))。
- Windows:每键 **Press + Release 双发**;`is_key_press` 现于 L138 逐事件滤 Release。`Ctrl+Enter=Enter+CONTROL`、`Shift+Enter=Enter+SHIFT`、`Ctrl+J=Char('j')+CONTROL`;裸提交/粘贴内部换行都是 `Enter modifiers=NONE`(同形)。`Event::Paste` 在 Windows 从不产。
- crossterm 同步 `event::poll` / `event::read` 与 `EventStream` **共用同一全局 internal reader**(`poll_internal`/`read_internal`),**不触碰 `EventStream` 的 waker 机制**。
- 既有 `on_key_inner` 已处理:换行键、纯 Ctrl+char 过滤(保 AltGr)、命令仅单行、硬/软模态、`Enter` 提交、换行分支同时 `refresh_command_completion()`(app.rs L974-978)。本 change 复用,不重写。

## D1 —— 批量 drain(同步 poll,**不用 now_or_never**)
主循环 `events.next()` 臂改为:
1. 从异步 `events.next()`(真 tokio waker)拿到第一个事件 `ev0`。
2. 同步抽干**已缓冲**事件:
   ```
   let mut batch = vec![ev0];
   while crossterm::event::poll(Duration::ZERO)? {
       batch.push(crossterm::event::read()?);
       if batch.len() >= CAP { break; }   // CAP 纯防御性上界
   }
   ```
   `?` 的 `io::Error` 按既有 `Some(Err(err)) => return Err(CliError::Io(err.to_string()))` 口径映射。
   **`CAP` MUST 取远超任何现实粘贴体量的大值(如 `1 << 20` 事件 ≈ 512K 字符)**,只作病态兜底:因 `poll(ZERO)` 抽干即返 false 自然收尾,已完整缓冲的粘贴会**一次抽干**、不被切成"尾-Enter 残余批";若 CAP 过小(如 4096≈2048 字符),一次约 50 行的粘贴会在 CAP 处被切,残余 `[Enter Press, Enter Release]` 落下一批 → 过滤 Release 后 `n==1` → **误判 Submit → 整段自动提交**(确定性触发本 change 要消除的 bug)。InsertStr 合并(D5)使超大批也 O(n) 不卡,故高 CAP 无性能代价。
3. 整批处理完,循环底 `terminal.draw()` **保持只一次**(一批一轮循环底 = 整批单次渲染)。

> **为何不用 `EventStream::next().now_or_never()`(审查 critical)**:`now_or_never` 以 **noop_waker** poll `EventStream`,会 CAS 翻转 crossterm 的 `stream_wake_task_executed` 并把后台 wake-task 绑到 noop waker;此后主 `select!` 再 poll `events.next()` 时见 flag 已 true → **跳过注册真 tokio waker** → 真实按键只 wake noop(无效),tokio 不被唤醒,输入被 120ms `spinner_tick` 心跳量化成持续卡顿。同步 `poll/read` 走 `poll_internal/read_internal`、不碰该 flag、不 spawn wake-task,**无此副作用**,下一轮 `events.next()` 照常注册真 waker。已对 crossterm 0.28.1 源码验实。

`ui_rx` / `spinner_tick` 仍由主 `select!` 后续轮次驱动(单次 drain 有界、抽完即回,不饿死)。

## D2 —— 批次分类(纯逻辑,TDD 核心)
### D2.1 先滤 Release、只算文本键
- 从 raw `batch: &[Event]` 抽 **Press 键事件**:`fn press_key_events(batch) -> Vec<KeyEvent>`(`filter is_key_press`)。**这是 load-bearing**:Windows 孤立 Enter = `[Enter Press, Enter Release]`,若把 Release 算进去 n 会翻倍、孤立 Enter 被误判 Newline → **提交永久失效**。
- **突发规模 `n` 只数落到输入层的"文本内容键"**:`Char`(非纯 Ctrl+char)+ **裸 Enter**。被前置守卫消费的键(`Ctrl+C`/`Esc`、`PageUp/PageDown`、`Up/Down` 归 picker/completion、`Home/End`…)与纯 modifier/控制键 **不计入 n**——否则 `PageUp` 后紧跟 `Enter` 同批会把 `Enter` 误判换行。

### D2.2 分类
```
enum KeyIntent { Newline, Submit, Passthrough }
fn classify_key_batch(keys: &[KeyEvent]) -> Vec<KeyIntent>
```
令 `n = keys.iter().filter(is_text_content_key).count()`(见 D2.1)。对第 i 个键:
- **裸 `Enter`**(`Enter && !CONTROL && !SHIFT`):`n >= 2` → `Newline`;`n == 1` → `Submit`。
- 其余(`Char`、`Enter+CONTROL/SHIFT`、`Char('j')+CONTROL`、光标键、Backspace、`Tab`…)→ `Passthrough`(原样交既有 `on_key`)。
- 分类器只接管**裸 Enter** 的"换行 vs 提交";modifier 版换行键仍走 `on_key` 换行分支。

## D3 —— 应用流(逐键 + 合并 Char)
对 raw batch 顺序处理:非 Key 事件(`Mouse`/`Resize`/`Focus`)按既有分支;Key(Press)按预算意图:
- **前置守卫仍逐键先行**(顺序不变):`should_exit` 命中即 `break`(批中途退出优先)、`handle_selection_key`、`arrows_route_*` / `handle_scroll_key`。消费掉的键不进输入层、不计入意图应用。
- 落到输入层的键:
  - **裸 Enter 且意图 `Newline`(突发)**→ `state.insert_newline_and_refresh()`(**短路**:插 `\n` + `refresh_command_completion()`,不走提交、也不被 `command_completion` 的 Enter=complete 截走——粘贴内容里的换行恒为换行)。
  - **裸 Enter 且意图 `Submit`**、及所有 `Passthrough` 非 Char 键 → `on_key_with_interrupt`(复用换行键/命令门/软模态)。
  - **连续 `Passthrough` 普通 Char**(粘贴正文,非纯 Ctrl+char)→ **合并成一次 `InsertStr(String)`** + 一次 `refresh_command_completion()`(见 D5 性能),而非逐字符 `InsertChar`。
- `insert_newline_and_refresh()` MUST 抽为 `AppState` 方法,**与 on_key 换行分支(app.rs L974-978)共用同一实现**,防两处副作用漂移。

## D4 —— 硬模态护栏(逐键按活跃态,pending 与 picker 分治)
突发意图(Newline 短路 / InsertStr 合并)MUST 只作用于**落到输入层**的键;硬模态活跃时键先被 on_key 内部模态分支消费、不进输入层。因模态态在批处理中会变(首键可能关闭它),故**逐键按当时活跃态**判定,而非预先整批截断:

对每个 Press 键(前置守卫链未消费者):
- `pending_permission` 活跃 → 走 `on_key_with_interrupt`(答 y/n/Enter/Esc),**随即 `break` 丢弃该批余下键**(一次权限只答一次;防粘贴余下内容漏进缓冲/连答)。
- `models_picker` 活跃 → 走 `on_key_with_interrupt`(`handle_models_picker_key` 自消费:`Char`→过滤、`Backspace`→退格、`Up/Down`→导航、`Enter`→选中并关闭、`Esc`→关闭)。**不截断、不套 burst 意图**(过滤输入 MUST NOT 丢失)。若此键使 picker 关闭,置 `modal_closed_in_batch`。
- 无硬模态 → 走 D3 输入层逻辑(Newline 短路 / InsertStr 合并 / on_key);但若 `modal_closed_in_batch` 已置且此键为裸 Enter → **丢弃**(防紧跟模态关闭键的粘贴尾 Enter 落进缓冲/提交)。

> **为何不整批截断到首键(修正 round-2 major)**:pending 是单次应答,截断合理;但 picker 是**打字过滤/导航**的多键面,大 transcript 慢渲染下 `Char*`/`Up-Down` 极易凑同批,整批截断会**静默丢过滤输入**("gpt4o" 只剩 'g')。故 picker MUST 每键透传给 `handle_models_picker_key`;`command_completion` 软浮层同理逐键透传。
> **burst 意图与模态次序**:classify 仍对全批算意图(纯逻辑),但**应用时按逐键活跃态**决定是否采用——硬模态活跃的键一律交 on_key(忽略其 burst 意图),只有真正落输入层的键才用 Newline/InsertStr;幸存裸 Enter 不会被误降级为换行绕过模态。

## D5 —— 性能:InsertStr 合并(推翻原"不引 InsertStr")
`reduce_input_buffer` 每个 action `state.clone()`(含 `text` + 整个 `input_history` Vec + `draft`)。逐字符 `InsertChar` 对大粘贴是 **O(n²)**(粘 n 字符,后段每字符 clone 增长中的缓冲 + 全历史)→ 大段粘贴单次 drain 上百 MB 拷贝、明显卡顿。
- **给 reducer 加 `InsertStr(String)`**:在光标处一次插入整串、一次 clone;批处理把**连续 Passthrough 普通 Char** 攒成一个 `InsertStr` 投递。clone 由 O(n²) 降到 ~O(text_len·drain数)。
- `InsertStr` 是纯逻辑 reducer 动作,单测(空串、含多字节 CJK、cursor 落 char 边界、插入后 cursor 到插入末尾、`history_cursor` 置 None 与 `InsertChar` 一致)。

## D6 —— 可测性边界
- **纯逻辑(red-green TDD)**:`press_key_events`(滤 Release)、`classify_key_batch`(D2)、硬模态截断判定(`fn hard_modal_key_limit(pending,picker)->Option<usize>`)、`InsertStr` reducer(D5)。均不依赖 `AppState`/异步,Mock 即测。
- **app 层单测**:对 `AppState` 直接投递"意图序列"(Char*+裸Enter(Newline)+…)验证行为(整批不提交、command_completion 经 Newline 后关闭、硬模态首键得 Allow),不依赖异步 drain。
- **TUI 外壳(test-after)**:`event::poll(ZERO)` drain 循环、draw 时机、`select!` 接线——既有 render 快照不churn + 手动真机复核。

## D7 —— 边界与失败模式(诚实记全)
- 空批 / 单键批:`n==1` 裸 Enter=Submit、其余单键=Passthrough(与今日逐键等价,零回归)。
- Release 已滤,不参与 n(D2.1)。
- `Ctrl+C`/`Esc` 在批中:`should_exit` 逐键前置,命中即 break。
- 批内 modifier 换行键:Passthrough → on_key 换行分支,照常换行。
- 硬模态:先截断,首键正常应答(D4)。

## D8 —— Non-Goals(启发式固有上限,诚实并列)
本 change 用 **batch-size 启发式**,不引 bracketed paste(Windows 无效),故以下**对称**误判均为已知上限、不在本 change 解决(需真实到达时序或 bracketed paste 才能根治):
- **慢/跨周期粘贴**:事件被拆到多个 poll 周期,某周期只含 1 个裸 Enter → 误判 Submit。
- **大 transcript 慢渲染下正常打字凑批**:batch = 上一轮 `terminal.draw()`(O(transcript))期间缓冲的事件;transcript 越大 draw 越慢、窗口越宽,则打字的末字符 + 紧随的提交 `Enter` 越易落同一 drain(n≥2)→ 提交 `Enter` 被误判换行(用户再按一次孤立 `Enter` 即提交)。正常大小 transcript(draw <16ms)几乎不触发;这是"时序被渲染污染 + Windows 无 bracketed paste"的根本取舍,batch-size 启发式无法根治。
- **粘贴以换行结尾**:`"a\nb\n"` 末尾 Enter 在突发批内 → 换行(不自动提交);符合"整段进缓冲不狂发"的目标。
- **粘贴含 Tab**:`KeyCode::Tab` 走 on_key `_ => {}`,不插 `\t`(与既有单键一致)——粘贴文本里的制表符会丢失,记为已知内容保真 gap,不在本 change 处理。
- **模态关闭后的粘贴尾 Enter**:批内某键关闭 picker/权限后、紧跟的粘贴裸 Enter 被丢弃(D4),不落缓冲——轻微内容丢失,接受。
- 不改 `terminal.rs`、不动 alt-screen/鼠标捕获/选区/滚轮;不引 `Event::Paste`。

> 注:CAP 取高值后,大粘贴一次抽干、不再被切成"尾-Enter 残余批",故"大粘贴自动提交"**不是** Non-Goal(已由高 CAP 消除)。
