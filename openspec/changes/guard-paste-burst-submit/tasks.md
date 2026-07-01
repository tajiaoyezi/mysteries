## 1. 批次分类 + InsertStr 纯逻辑(TDD)

- [ ] 1.1 **RED**:新建批次分类模块(如 `src/tui/input_batch.rs`),只写失败测试:
  - `fn press_key_events(batch: &[Event]) -> Vec<KeyEvent>`:滤除非 Press(Windows Press+Release 双发,**Release MUST 不计入**)。
  - `fn classify_key_batch(keys: &[KeyEvent]) -> Vec<KeyIntent>`(`enum KeyIntent { Newline, Submit, Passthrough }`):`n` = **文本内容键**(`Char` 非纯 Ctrl+char + 裸 `Enter`)计数;`n>=2` 时裸 `Enter`→`Newline`、`n==1` 时裸 `Enter`→`Submit`;`Char`/光标键/`Backspace`/`Tab`/`Enter+CONTROL/SHIFT`/`Char('j')+CONTROL`→`Passthrough`;被前置守卫消费的键(`PageUp/PageDown`/`Ctrl+C`/方向键等非文本键)**不计入 n**。
  覆盖:①`[Enter Press, Enter Release]`→press 后 n=1→`Submit`(Release 不翻倍);②`[Char a, Enter, Char b]`→裸 Enter=`Newline`;③孤立 `[Enter]`→`Submit`;④`[PageUp, Enter]`→n=1(PageUp 不计)→`Submit`;⑤`Enter+CONTROL`→`Passthrough`;⑥空批→空。运行确认**失败原因正确**(非编译错),贴测试 + 红输出 → **停,等确认**(新接口首次成型)。
  > 硬模态(pending/picker)分治属**应用层逐键逻辑**(依赖 AppState 演进态),不进此纯函数,见 2.3。
- [ ] 1.2 **GREEN**:最小实现让 1.1 全绿(仅按"Press-only 文本内容键数 + 是否裸 Enter"判定,不看时间戳)。登记进 `mod.rs`。
- [ ] 1.3 `input_buffer.rs` 加 `InsertStr(String)` 动作(red-green):在光标处一次插入整串、cursor 到插入末尾且落 char 边界、`history_cursor` 置 None;与逐个 `InsertChar` 得相同 `text` 但只 clone 一次。补单测(空串、含多字节 CJK、cursor 中途插入)。

## 2. 事件循环批量 drain + 应用流(mod.rs / app.rs)

- [ ] 2.1 主循环 `events.next()` 臂:从异步 `events.next()` 拿首事件 `ev0` 后,**同步**抽干 `while crossterm::event::poll(Duration::ZERO)? { batch.push(event::read()?); if batch.len() >= CAP { break; } }`(**`CAP` 取 `1 << 20` 纯防御性上界**——正常粘贴一次抽干不触及;若取小值如 4096,~50 行粘贴会被切、残余尾 Enter 单独成批被误判 Submit 自动提交;`?` 的 io error 按既有 `CliError::Io(err.to_string())` 映射)。**不用 `now_or_never`**(避免毒化 EventStream waker → 120ms 卡顿)。循环底 `terminal.draw()` **保持只一次**。`ui_rx`/`spinner_tick` 仍由主 `select!` 后续轮次驱动。
- [ ] 2.2 `app.rs` 加 `insert_newline_and_refresh()`(插 `\n` + `refresh_command_completion()`),**重构 on_key 换行分支(L974-978)改用它**,批 apply 与 on_key 共用同一实现防漂移。
- [ ] 2.3 批 apply(mod.rs):`classify_key_batch` 对全批 Press 键(`press_key_events`)算意图(纯逻辑,全批算不受模态影响);逐 raw 事件顺序处理——`Mouse`/`Resize`/`Focus` 走既有分支;Key(Press)先过前置守卫链(`should_exit` 命中即 `break`、`handle_selection_key`、`arrows_route_*`/`handle_scroll_key`)。守卫未消费者**按当时活跃态逐键分治**(D4):
  - `pending_permission` 活跃 → `on_key_with_interrupt` 后 **`break`**(答一次、丢余下);
  - `models_picker` 活跃 → `on_key_with_interrupt`(picker 自消费过滤/导航/选中,**不套 burst 意图、不合并 InsertStr**);若此键使 picker 关闭,置 `modal_closed_in_batch`;
  - 无硬模态 → 按意图:`modal_closed_in_batch` 且裸 Enter → 丢弃;否则 `Newline`(裸 Enter 突发)→ `insert_newline_and_refresh()`(短路,不提交、不被 completion Enter=complete 截走);`Submit`/`Passthrough` 非 Char → `on_key_with_interrupt`;**连续 `Passthrough` 普通 Char** → 攒成一次 `InsertStr` + 一次 `refresh_command_completion()`。
  补 app 层单测:①一批 `Char*+裸Enter+Char*` 只入缓冲、不提交、不发 Prompt;②`command_completion` 开着时批内 `Newline` apply 后 completion 关闭(None);③`pending_permission` 活跃 + 首键裸 Enter 的 ≥2 键批 → 恰一次 Allow、随即 break、缓冲无杂散 `\n`;④`models_picker` 活跃 + 同批多个过滤 `Char`(如 "gpt")→ **全部进 `push_filter_char` 不丢**(不被截断);⑤picker 中途 Enter 选中关闭后、同批尾随裸 Enter → 丢弃、不提交。

## 3. 校验 + 真机

- [ ] 3.1 `cargo test --lib` 全绿 + `cargo clippy --all-targets -- -D warnings` 零警告 + `openspec validate guard-paste-burst-submit --strict` 过 + **既有 render 快照零 churn**(delta 含 ADDED + MODIFIED「多行输入编辑」,MODIFIED 已整条复述)。
- [ ] 3.2 **真机复核**(Windows Terminal):粘贴多行内容 → **整段进输入框不发送**、内部换行保留;手按 `Enter` → 提交;`Ctrl+Enter`/`Shift+Enter`/`Ctrl+J` 仍换行;粘贴内容**撞权限询问**时不连答 Allow、首键正常应答;`/` 补全下粘贴/连打仍过滤不误提交;`Ctrl+C`/`Esc` 退出/中断即时;粘贴**超大段不卡死**(InsertStr 合并);**飞快连续打字无逐字符延迟**(验证无 now_or_never waker 回归);既有选区/滚轮/滚动/历史不回归;**已知 Non-Goal(慢/跨周期粘贴、飞快打字凑批、粘贴末尾换行不自动提交、粘贴含 Tab 丢失)不阻塞**。
