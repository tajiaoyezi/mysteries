# Tasks — add-session-picker-and-exit-guard

红灯纪律:红灯独立成步,以断言失败落红(非编译错)——新类型/新签名允许红灯内先落桩。**红灯停点**:3.1 为 `SessionPicker` 新 modal 接口首次成型,测试 + 失败输出贴出后**停下等确认**再进绿灯;1.x / 2.x / 4.x / 5.x 可连写。
执行 agent MUST NOT:git 写操作、修改既有快照/夹具以过测(**例外见 4.1**:删 655 令 `should_exit(ready, ctrl_c)` 断言合法翻红,须更新而非绕过)、勾选第 6 节真机任务、**全仓 `cargo fmt`**(只碰你改的文件)、kill 用户进程。

## 1. exit-intent 纯函数(强制 TDD)

- [x] 1.1 红→绿:`exit_intent_action(gap: Duration, threshold: Duration) -> ExitIntent { Consumed, Exit }`(**2 态**,审查 M-1:签名只有 gap/threshold、无 key/state 产不出 `NotHandled`;排除项由调用方 gate,对齐 `cancel_action` mod.rs:571 的 2 态范式)——`gap < threshold` → `Exit`,`gap >= threshold`(含边界 `==`)→ `Consumed`;常量 `EXIT_DOUBLE_TAP = 1s`;`AppState` 加 `last_exit_intent_at: Option<Instant>` + getter/setter(仿 `last_cancel_at` app.rs:433/518/522)。测试:窄 gap → Exit、宽 gap → Consumed、边界 gap==threshold → Consumed

## 2. list_sessions(强制 TDD)

- [x] 2.1 红→绿:`SessionStore::list_sessions() -> io::Result<Vec<SessionSummary>>`,`SessionSummary { id: String, created_at: String, first_user: Option<String> }`——扫 `sessions/*.jsonl`(仿 `latest` mod.rs 87-119),每文件读 `Meta` 取 `id`/`created_at` + 逐行找首个 `Msg(User(_))` 截断(≤60 字符);**mtime 逆序**;**损坏文件跳过、不整体失败**(列表容错,区别 `load` 严格 `Err`);非 `.jsonl` 忽略。测试:多会话 mtime 逆序、首 `User` 截断、无 `User` → `None`、损坏跳过其余正常、空目录 → 空 `Vec`、**非 `.jsonl` 文件忽略**(审查 L3)

## 3. SessionPicker(强制 TDD)

- [x] 3.1 红(**停点**):`SessionPicker { rows: Vec<SessionRow>, highlighted: usize }` + `SessionRow { id, label }`(仿 `ModelsPicker` app.rs:127);`new(summaries)`(label = 短 id 前 8 + `created_at` + `first_user`)、`move_highlight(delta)`(边界钳制)、`selected() -> Option<&str>`;`AppState.session_picker: Option<SessionPicker>` + `open_session_picker`/`close_session_picker` + `pending_session_switch: Option<String>` + `take_pending_session_switch()`;`handle_session_picker_key(key)`——`Up`/`Down` 移高亮、`Enter` → 置 `pending_session_switch` + close、`Esc` → close、**其余键 catch-all consume**(审查:仿 ModelsPicker `_ => true` app.rs:917,防字符漏进输入框)。测试(断言红):`new` label、`move_highlight` 钳制、`Enter` 置 switch、`Esc` 清 picker 不置 switch、**catch-all 吞任意字符键**
- [x] 3.2 绿:最小实现

## 4. TUI 接线(IO 胶水,事后回归)

- [x] 4.1 Ctrl+C 分两态(审查 H1/H-2:现状 running+idle 都退出、中断只 Esc;改为 running 中断 / idle 双击退出):**running+Ctrl+C 中断经扩 `app.rs:1109` 中断条件为 `(Esc || Ctrl+C) && phase.is_running()`**(审查 H-2 省事解:**不**在 `process_event_batch` 单独接线——那会抢在 selection-copy@1150 / queue-cancel@1154 前,`is_running()` 与 `has_queue()` 可并存;扩 1109 自动继承 Esc 正确次序);`should_exit`(mod.rs:655)**裸 Ctrl+C 尾表达式替换为 `false`**(审查 L-1:只删则函数无尾表达式、不编译);`process_event_batch`(1138 循环内、`should_exit`@1145 前)——**idle + Ctrl+C → `exit_intent_action`**(排除项由调用方 gate:selection/queue/permission/completion/running/picker 于前置分支已消费):`Consumed` → `set_last_exit_intent_at` + 提示、consume 键;`Exit` → `break_loop`。**显式更新既有断言 `mod.rs:2082`**(`should_exit(ready, ctrl_c)` 由 `true` 改 `false`——**合法行为变更、豁免「保绿/禁改夹具」**,审查 M1;`mod.rs:2083` Esc idle-退出断言保持绿)。加断言:idle 首按不退 + 提示、阈值内再按退、超时重置、running Ctrl+C 中断不退
- [x] 4.2 SessionPicker **early route**(审查 HIGH-3 Esc 误退 + H-1 落点 + MEDIUM catch-all/arrows):`process_event_batch` 事件循环里 **`press_index += 1`(mod.rs:1143)之后、`should_exit`(1145)之前**(审查 H-1:放 `press_index++` **前**则 picker consume 键时 index 不进、batch 内 picker 中途关闭后尾随 press 键错位分类;picker consume 的键**仍照常 `press_index += 1`**)——`if state.session_picker.is_some() { handle_session_picker_key(key); continue }`,**吃每一个键**。一处 early route 同时消解:Esc 被 `should_exit`(mod.rs:651 无 session 守卫)误退、Up/Down 走 `handle_scroll_key` 滚 transcript、字符漏输入、Ctrl+C 归 picker。`render.rs` 加 `session_picker` 渲染(仿 `render_models_picker`)+ 新快照一张、既有零 churn
- [x] 4.3 hot-swap(审查 H2 补 `state.session`):`handle_session_picker_key` 的 `Enter` 只置 `pending_session_switch`(同步);`run_tui` **`select!` 块结束后、`draw_frame`(mod.rs:291 前、async loop body 可 `.await`)** `if let Some(id) = state.take_pending_session_switch()` → `store.load(id)`(失败 → `Notice`)→ `replace_system_head` → `*state.agent_history.lock().await = history` + `state.transcript = transcript` + `input_tx.send(SetProvider{meta.provider.clone(), meta.model.clone()})` + **`state.session.provider = meta.provider.clone(); state.session.model = meta.model.clone()`**(审查 H2:`write_session_snapshot` mod.rs:355 与 footer render.rs:1332 读 `state.session`,漏则续写污染 + footer 错;镜像 app.rs:894-895;审查 L-2:`.clone()` 防 `session_meta = meta` partial-move)+ `session_meta = meta`(**`let mut`** mod.rs:81,续写选中文件)
- [x] 4.4 启动接入:`run_tui` `resume` → **新会话 startup + spawn + loop 内 `open_session_picker(store.list_sessions())`**(空列表 → 不弹、按新会话);`--continue` → `prepare_session_startup` load-latest(D6);`main.rs` real_main 解析 `--resume`/`--continue` → **纯函数 `startup_mode(resume: bool, continue_: bool) -> StartupMode { Fresh, Resume, Continue }` + 单测**(审查 M1:arg 映射可纯函数化、应 TDD——both→Resume、单 resume→Resume、单 continue→Continue、无→Fresh;`--resume` 优先),经 `run_tui(paths, StartupMode)` 传入(替 `resume: bool`);`--headless` 不受影响
- [x] 4.5 活动行提示:`render.rs` 活动行加「再按一次 Ctrl+C 退出」(`last_exit_intent_at` 未过期时);**优先级 exit-intent > copy_hint > paste-receiving**(审查 MEDIUM-4:exit-intent 上膛警告不被 copy_hint 遮,否则守卫失效);过期由 120ms spinner tick 驱动消失(仿 copy_hint TTL)。**补 insta 快照:copy_hint 与 exit-intent 同时置位 → 活动行显 exit-intent**(审查 M2:该优先级是守卫失效的修复点,需回归护栏)

## 5. 门禁

- [x] 5.1 `cargo test --lib` 全绿(含 4.1 更新后的 `should_exit` 断言);`cargo clippy --all-targets -- -D warnings` 零警告;快照仅预期新增、既有零 churn
- [x] 5.2 `openspec validate add-session-picker-and-exit-guard --strict` 通过
- [x] 5.3 `cargo build` 通过(若 exe 被占 os error 5 报告即可、别 kill)

## 6. 真机核验(主 agent / 用户;执行 agent MUST NOT 勾)

- [x] 6.1 idle Ctrl+C 连按:空闲单次 → 不退 + 活动行提示;1s 内再按 → 退出;>1s 后单按 → 又只提示(重置)
- [x] 6.2 `--resume`:进 TUI 弹会话列表,↑↓ 选、Enter → 对话 + 工具卡 + **provider/model 还原(footer 正确、续写不污染)**;Esc → 停新会话;**选中 `meta.provider` 已从配置移除的会话 → 回退默认 + `Notice`、不 panic**(审查 L1)
- [x] 6.3 `--continue`:直接续最近、不弹 picker
- [x] 6.4 Ctrl+C 语义:**agent 运行中 Ctrl+C → 中断当前轮(不退)**、与 Esc 一致;selection 态 Ctrl+C 复制;queue 态清队;冷启动(无 flag)不变
