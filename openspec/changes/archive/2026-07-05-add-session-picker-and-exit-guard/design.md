# Design — add-session-picker-and-exit-guard

> 经三路对抗审查修订:running+Ctrl+C 语义(H1)、hot-swap 补 `state.session`(H2)、picker 键 early route(H3/键路由重灾区)、exit-intent 三态 + 提示优先级、行号校正——详见各决策注。

## 决策

### D1 Ctrl+C = running 中断 / idle 双击退出(拆两态)
- **现状**:`should_exit`(mod.rs:655)裸 Ctrl+C(running **或** idle)无条件 `true` → 退出;中断当前轮**只认 Esc**(app.rs:1109 `Esc && phase.is_running() → send Interrupt`)。
- **审查修订(H1,我原 spec/tasks 误述「running Ctrl+C 中断」)**:现状 running Ctrl+C 是退出、非中断;定案对齐 Esc + Claude Code——
  - **running + Ctrl+C → `Interrupt`**(审查 H-2 省事解):**扩 `app.rs:1109` 中断条件为 `(Esc || Ctrl+C) && phase.is_running()`**——**不在 `process_event_batch` 单独接线**(那会抢在 selection-copy@1150 / queue-cancel@1154 之前,`is_running()` 与 `has_queue()` 可并存、type-ahead 排队);扩 1109 自动继承 Esc 的正确次序(在 selection/queue handler 之后)。
  - **idle + Ctrl+C → exit-intent 双击**:首次 `set_last_exit_intent_at(now)` + 活动行提示「再按一次 Ctrl+C 退出」、**不退**;`EXIT_DOUBLE_TAP`(1s)内再按 → 退出;超时未再按 → 提示消失、重置。
  - `should_exit`(mod.rs:655)**裸 Ctrl+C 尾表达式替换为 `false`**(审查 L-1:只删则函数无尾表达式、不编译);idle exit-intent 在 `process_event_batch` 接线(排除项 gate 后),running 中断经扩 1109。Esc 保留:running 中断、idle 单次退出(should_exit Esc 分支)。
- **纯函数**(强制 TDD):`exit_intent_action(gap, threshold) -> ExitIntent { Consumed, Exit }`——**2 态**(审查 M-1:签名只有 gap/threshold、无 key/state,产不出 `NotHandled`;排除项改由**调用方 gate**,对齐 `cancel_action` 范式):`gap < threshold → Exit`(阈值内连按),`gap >= threshold`(含 `==`)→ `Consumed`(首次/超时:记时 + 提示 + 消费键)。`EXIT_DOUBLE_TAP = 1s`;`AppState` 加 `last_exit_intent_at`(仿 `last_cancel_at` app.rs:433/518/522)。
- **排除项**:idle-exit-intent 仅在**空闲态**介入——selection(复制)/queue(清队)/permission/completion/**running(中断)**/**picker** 时不介入(各有既定处理)。排除列表须与 `should_exit` 现有前置分支同门。
- **提示优先级(审查 MEDIUM-4)**:活动行 **exit-intent > copy_hint > paste-receiving**——exit-intent 上膛时其警告**不被 copy_hint 遮**(否则「有选区复制→copy_hint→idle Ctrl+C 上膛被遮→再按直接退、用户没见警告」使守卫失效)。`render.rs:1172` 活动行链改为先判 exit-intent。

### D2 SessionPicker(A1) + 键 early route
- **选**:`SessionPicker { rows: Vec<SessionRow>, highlighted }` + `SessionRow { id, label }`(仿 `ModelsPicker` app.rs:127);`AppState.session_picker: Option<SessionPicker>` + `open_session_picker`/`close_session_picker`;`handle_session_picker_key`(`Up`/`Down` 移高亮、`Enter` → 置 `pending_session_switch=Some(id)`+close、`Esc` → close、**其余键 catch-all consume**)。渲染仿 `render_models_picker`(render.rs)。
- **键路由 = early route(审查 HIGH-3 picker Esc 误退 + H-1 落点 + MEDIUM catch-all/arrows)**:picker 打开时,`process_event_batch` 事件循环里 **`press_index += 1`(mod.rs:1143)之后、`should_exit`(1145)之前**——`if state.session_picker.is_some() { handle_session_picker_key(key); continue }`,**吃掉每一个键**。**落点必须在 `press_index += 1` 之后**(审查 H-1:放其前则 picker consume 键时 `press_index` 不进,batch 内 picker 中途关闭后尾随 press 键读到偏小 `intents[press_index]`、静默错分类)——picker consume 的键**仍照常 `press_index += 1`**。一处 early route 同时消解:①Esc 被 `should_exit`(mod.rs:651,无 session_picker 守卫)吃掉误退出;②Up/Down 走 `handle_scroll_key` 滚 transcript(现靠 `arrows_route_to_models_picker` mod.rs:658 拦,session 版无);③字符键漏进输入框;④Ctrl+C 归 picker。
- **弃**:分散镜像 ModelsPicker 的多处守卫(`should_exit` Esc 前置 629 + `arrows_route_to_models_picker` 658 + `apply_batch_input_key` 1293 分支)——易漏(审查逐条点出);early route 集中、健壮。

### D3 hot-swap = idle lock 替换 + 补 state.session
- **前提**:`agent_history`(`Arc<Mutex>`,102 建 / 116 clone 进 spawn / 132 move 进 state,**同一 Arc**)锁仅在 `run_agent_task` 处理轮(mod.rs:**1248/1258/1267/1278**)与 `run_tui` `ui_rx` 臂落盘(mod.rs:**273**)持有;`--resume` 走**新会话** startup(D5)→ idle 弹 picker 时 agent 阻塞 `input_rx.recv()`(1230)、无进行中轮、ui_rx 无终止事件 → 锁空闲,hot-swap 安全。`select!` 每轮单臂串行,events 臂与 ui_rx 臂不并发。
- **选**:`Enter` 选中 `id` → `handle_session_picker_key` 置 `state.pending_session_switch`(同步、零 async);`run_tui` 主循环 **`select!` 块结束后(收于 mod.rs:290)、`draw_frame`(293)前的 291 处**(async loop body、可 `.await`)`if let Some(id) = state.take_pending_session_switch()` 执行:
  1. `store.load(id)` → `(meta, mut history, transcript)`(失败 → `Notice`、不崩);
  2. `replace_system_head(&mut history, DEFAULT_SYSTEM_PROMPT)`;
  3. `*state.agent_history.lock().await = history`;
  4. `state.transcript = transcript`;
  5. `input_tx.send(UserInput::SetProvider{ id: meta.provider.clone(), model: meta.model.clone() })`(agent 侧还原,复用其 profile 查找 + 缺失回退 Notice);
  6. **`state.session.provider = meta.provider.clone(); state.session.model = meta.model.clone()`(审查 H2:落盘 `write_session_snapshot` mod.rs:355 与 footer render.rs:1332 都读 `state.session`——漏则续写把选中会话 `meta.provider` 覆盖成 startup 默认、footer 显错;镜像 app.rs:894-895。审查 L-2:`.clone()` 防 step 7 `meta` partial-move)**;
  7. `session_meta = meta`(**`let mut`**,mod.rs:81;续写选中 `<id>.jsonl`)。
- **跨 change 引用**:`replace_system_head`(add-session-persistence D8)、`SetProvider` 通道(其 D9)复用。

### D4 list_sessions = SessionStore 枚举 + Meta + 首 User 摘要
- `SessionStore::list_sessions() -> io::Result<Vec<SessionSummary>>`,`SessionSummary { id, created_at, first_user: Option<String> }`——扫 `sessions/*.jsonl`(仿 `latest` mod.rs 87-119),读 `Meta` 取 `id`/`created_at` + 逐行找首个 `Msg(User(_))` 截断(≤60 字符);mtime **逆序**;损坏文件跳过、不整体失败(列表容错,区别于 `load` 严格 `Err`);非 `.jsonl` 忽略。`SessionRow.label` = 短 id(前 8)+ 时间 + 首 User 摘要。

### D5 --resume 启动接入 = 进 TUI 弹 picker(不 pre-spawn load)
- **选**:`run_tui` `resume` → 以**新会话** startup(空 seed)进 TUI + spawn,进 loop 后立即 `open_session_picker(store.list_sessions())`;选 → hot-swap(D3);`Esc` 取消 → 停新会话。`list_sessions` 空 → 不弹、按新会话。非 resume/continue → 冷启动不变。
- `--resume` 让语义从「续最近」变「列出选」;`prepare_session_startup` load-latest 分支改由 `--continue` 复用(D6)。
- **note(审查 L4:空会话不积累)**:`--resume` 的新会话 startup **未对话即 hot-swap 到历史会话时不触发落盘**(落盘挂终止事件,add-session-persistence D3)——新会话 id 不产生空 `.jsonl`、不污染下次 picker。仅当用户 `Esc` 取消停在新会话**并对话**才落盘该新会话(正常)。

### D6 --continue = 续最近(复用 prepare_session_startup load-latest)
- `--continue`(无参):进 TUI 前 `prepare_session_startup` load latest(= add-session-persistence 的 `--resume` 旧语义:load + `replace_system_head` + provider 还原经 profile 查找 + 缺失回退 Notice)、不弹 picker。`--resume` 与 `--continue` 同给 → `--resume` 优先。无历史会话 → 新会话。CLI 传参 `run_tui(paths, StartupMode { Fresh, Resume, Continue })`(替 `resume: bool`)。

## 数据格式
`SessionSummary` 仅内存(list 用),不落盘;会话文件格式沿用 add-session-persistence(Meta/Msg/Block jsonl)。

## 接缝(实现挂载点)
- **Ctrl+C**:`should_exit`(mod.rs:655)删裸 Ctrl+C;`process_event_batch`(1138 循环内、should_exit@1145 前)插:running Ctrl+C → `interrupt_tx.send(Interrupt)`;idle Ctrl+C → `exit_intent_action` → Consumed(记时+提示)/Exit(break_loop)。`exit_intent_action`/`ExitIntent`/`last_exit_intent_at` 新增;`render.rs:1172` 活动行 exit-intent 优先。
- **SessionPicker**:`app.rs` 加类型/字段/`open`/`handle_session_picker_key`(catch-all)/`pending_session_switch`+`take_`;`render.rs` 加渲染;`process_event_batch` 循环最前加 session_picker early route。
- **hot-swap**:`run_tui` `select!` 块后(mod.rs:291 前)消费 `take_pending_session_switch` → async load + 六步替换(含 state.session)+ session_meta(`let mut`)。
- **list_sessions**:`session/mod.rs` 加。
- **启动/CLI**:`run_tui` resume 分支改新会话 startup + loop 内 open picker;`main.rs` real_main 解析 `--resume`/`--continue`(互斥,resume 优先)→ `StartupMode`;`run_tui(paths, StartupMode)`。

## 风险 / 权衡
- **running Ctrl+C 变中断**:改破坏性动作(退出)为中断,更安全;既有测试 `should_exit(ready, ctrl_c)==true`(mod.rs:2082)必翻红——**合法更新**(行为确变),tasks 显式豁免、不算「造假过测」。
- **hot-swap idle 假设**:仅启动即弹 picker(agent 未收 prompt)hot-swap;picker 吃 Enter 故 picker 期间不提交 prompt,锁空闲。
- **提示争用**:exit-intent > copy > paste 单选渲染。
- **two double-tap**:`last_exit_intent_at` 与 `last_cancel_at` 独立 + `has_queue` 互斥(审查判定不串扰)。

## 定案(用户拍板)
1. running + Ctrl+C → **中断**(对齐 Esc + Claude Code)。
2. hot-swap 后**续写选中 `<id>.jsonl`**、且**补 `state.session`**。
3. `EXIT_DOUBLE_TAP` = **1s**;`--resume` 仅 1 会话仍弹 picker;本轮加 `--continue`。
4. 键路由 = **early route**(picker 打开吃所有键、先于一切)。
