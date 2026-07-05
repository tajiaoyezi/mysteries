# Design — add-session-persistence

> 本文经两轮对抗审查修订(第一轮 3 路初审 + 第二轮 2 路复审):落盘接缝(D3,两轮纠正落点)、provider 还原(D9,改经 SetProvider channel)、System(D8)、compact/clear 语义均已定案,详见各决策的「审查修订」注。

## 决策

### D1 持久化范围 = `SessionMeta` + `agent_history` + `transcript`(完整还原)
- **选**:落盘三者——`SessionMeta`(provider/model 等)、`agent_history: Vec<Message>`(续跑权威)、`transcript: Vec<TranscriptBlock>`(UI 视觉,含 `ToolCard` 运行态)。恢复后对话、工具卡、provider/model 一并还原。
- **弃**:仅存 `agent_history`、transcript 重建(工具卡降级为摘要)——用户要求完整 UI 保真。
- **依据**:用户拍板(完整 UI 状态 + 对标 Claude Code)。resume 时 System 消息特殊处理见 D8。

### D2 存储 = jsonl 行式 tagged record + 每轮全量重写
- **选**:一会话一 `.jsonl`;每行一个 `SessionLine` tagged enum:`Meta(SessionMeta)` / `Msg(Message)` / `Block(TranscriptBlock)`。保存 = 写 `Meta` 行 + `agent_history` 逐条 `Msg` 行 + `transcript` 逐块 `Block` 行,**全量重写整文件**。加载 = 逐行 parse、按 tag 分派回三容器(**恰一 `Meta` 行**;缺失或多 `Meta` → `Err`)。
- **弃**:① Claude Code 式 append 事件溯源(mysteries 是双结构,需统一事件模型 + replay 引擎,独立大重构,超本轮);② 单 JSON 对象(不利逐行 diff);③ SQLite(违内核自实现最简解、引入重依赖、无并发)。
- **依据**:全量重写在双结构 + compact 下最简正确(compact 缩短 `history` 时重写自然反映,append 无法表达删除);会话量级(数十~数百 KB)下每轮重写可接受;保留 jsonl 逐行观感(人可读、已 flush 行崩溃后有效)。

### D3 落盘时机 = 事件循环 async 调用点终止事件全量重写(容错不阻断)
- **选**:落盘挂在 `run_tui` **异步事件循环的 `ui_rx.recv()` 臂**(tui/mod.rs:250-259,本就在 `run_tui` async 体内)。`handle_agent_event` **保持同步不动**;在其**调用前**按引用算 `is_terminal`(复刻邻近 `reassert_mouse_capture` 的 by-ref `matches!`,246-249),**调用后**:`if is_terminal { let h = state.agent_history.lock().await; if store.write(&meta, &h, &state.transcript).is_err() { drop(h); state.transcript.push(Notice) } }`。`store` / `meta` 为 `run_tui` async 体局部持有。写失败 → 推 `Notice`,**不阻断对话与 `agent_history`**。
- **审查修订(第一轮 H1 → 第二轮 HIGH-1)**:
  - **第一轮 H1**:原设计挂 `run_agent_task`——该任务是独立 `tokio::spawn`、入参只有共享 `agent_history`,**拿不到未共享的 `transcript`**,故保存须在事件循环侧。
  - **第二轮 HIGH-1(纠正第一轮的落点)**:但**不能挂进 `handle_agent_event` 函数体**——它是同步 `fn`(mod.rs:284),而 `agent_history` 是 tokio `Mutex`(app.rs:405,读需 `.lock().await`),**同步 fn 内 `.await` 编译不通过**。改 `async fn` 会连带 `feed_agent_event` test helper + 4 处同步 `#[test]`;`try_lock()` 也不行(`CompactDone` 在 run_agent_task **持锁**发出 1148-1153,`try_lock` 必失败 → 静默丢 compact 快照)。故落在 **async 调用点**(循环体),`handle_agent_event` 保持同步(**零测试面波及**)。`CompactDone` 用 `.lock().await` 正确:等 run_agent_task 于 1153 `drop` guard(其间无 await、不死锁),读到的正是 1149 就地压缩后的 history。四终止事件(`TurnComplete`@1170 / `Error`@1173 / `Interrupted`@1179 于锁释放后发;`CompactDone`@1152 持锁发、async 等待可解)全覆盖且快照正确。
- **弃**:仅退出时落盘(崩溃丢整场);挂 `handle_agent_event` 函数体内(同步 fn 不能 await);`try_lock`(丢 compact 快照)。

### D4 session id = `uuid` v4
- **选**:`uuid` crate(feature `v4`),`id: String`。
- **依据**:用户拍板对标 Claude Code。新依赖 `uuid` 理由 = 会话标识 + 全局唯一;`id` 为 `String`、不需 `uuid` 的 serde feature。

### D5 存储位置 = `<config_dir>/sessions/<uuid>.jsonl`
- **选**:复用 `~/.config/mysteries/`(config_dir),其下 `sessions/` 平铺。
- **审查修订(L5)**:`CliPaths`(cli.rs:24-30)当前**无 `config_dir` 字段**(仅 user_config/project_config/credentials/cwd);config_dir 是 `default_paths()`(main.rs:72)局部量。需给 `CliPaths` 加 `config_dir`(在 `default_paths` 填充),`sessions/` = `config_dir.join("sessions")`。
- **弃**:按项目路径编码分目录(CC 做法)——本轮取全局最近;`SessionMeta.cwd` 已记录、为项目隔离留钩。

### D6 恢复入口 = `--resume`(无参 = 最近会话)
- **选**:CLI flag `--resume`,加载 `sessions/` 下 mtime 最新的 `.jsonl`;注入 `history`(System 处理见 D8)+ `transcript`,并按 D9 还原 provider。
- **弃**:`--resume <id>`、TUI 内 `/resume`、交互式列表(均后续)。
- **审查修订(L6)**:`run_tui(paths: CliPaths)`(tui/mod.rs:64,唯一调用 main.rs:47)需加 `resume: bool` 参或塞进 `CliPaths`,低 blast-radius;`main.rs` real_main(43-48)dispatch 分支解析并剥离 `--resume`。

### D7 序列化扩展 = UI 类型加 `derive`
- `TranscriptBlock`(app.rs:41)/ `ToolCard`(app.rs:91)/ `ToolCardStatus`(app.rs:84)/ `StatusSnapshot`(app.rs:73)加 `Serialize` / `Deserialize`;既有 `Clone`/`Debug`/`PartialEq`/`Eq` 不动。`ToolCard.args: Value`、`Message` / `ToolCall` 已可序列化。以 round-trip 单测锁定。

### D8 resume 时 System 消息 = 替换为当前 `DEFAULT_SYSTEM_PROMPT`(主 agent 定,可否决)
- **选**:加载 history 后,以当前 `DEFAULT_SYSTEM_PROMPT` 作 `history[0]` 的 `System`(丢弃持久化的旧 System),其后接加载的 `User`/`Assistant`/`ToolResult` 对话内容。**空-history 守卫(审查 LOW-5)**:经 `history.first_mut()` 判空、而非直接下标 `history[0]`——store 层允许空 history round-trip(手工/损坏文件),空时跳过替换、不 panic。
- **弃**:逐字还原持久化旧 System——版本升级改了 prompt 后 resumed 会话仍跑旧 prompt(陈旧)。
- **依据**:System 是产品身份/工具契约,应 always fresh;仅对话内容属会话状态。顺带消解 `SessionMeta.app_version` 的迁移问题(prompt 不锁版本)。`app_version` 本轮仅记录、不做 load 端校验(留未来兼容用)。
- **审查来源**:agent2 M4。

### D9 provider 还原 = spawn 后经 `SetProvider` channel 注入
- **选**:resume 时,事件循环在 agent task **spawn 之后**经 `input_tx` 注入一条 `channel::UserInput::SetProvider { id: meta.provider, model: meta.model }`,复用 `run_agent_task` 现成的 `SetProvider` 臂(tui/mod.rs:1133-1145)——其内部即 `apply_set_provider`(1195-1234:`profiles.get(id)` → transient `Config`(`meta.model`)→ `select_provider` → `set_provider`/`set_model`);profile 缺失(1203)/凭据缺失(1208)时其 `Err` **已自动 `ui_tx.send(Notice)`**(1143)。
- **审查修订(第一轮 agent1 M2/M3 → 第二轮 MEDIUM-2)**:
  - 第一轮:`meta.provider`(仅 id)不足以直接喂 `select_provider`(需 kind/base_url),须经 profile 查找;不得复用 `resolve_active_provider`(app.rs:163-199 静默 remap、回退首个 profile、**不发 Notice**)。
  - 第二轮:**不在 run_tui 前段直接仿 `apply_set_provider`**——`agent` 在 mod.rs:104 已 `move` 进 spawn(其后无 agent 可改)、`state` 到 114 才构造(Notice 无处推)。经 `SetProvider` channel 注入绕开两墙:agent 归任务所有、Notice 走既有通道、profile 查找 + 回退 **复用已测的 `SetProvider` 臂**(零新增回退测试面)。
- **弃**:run_tui pre-spawn 仿 apply_set_provider(agent move / state 未建两墙);复用 `resolve_active_provider`(不发 Notice)。
- **依据**:`SetProvider` 臂 + 其 `Notice` 是现成、已测通道;resume 只注入一条,回退分支免新单测。

### D10 `/clear` desync = 如实快照,既有 desync 另案(主 agent 定,可否决)
- **背景**:`/clear`(实处理 `src/tui/app.rs:1201-1202` `Command::Clear => { self.transcript.clear(); }`)只清 `transcript`、不清 `agent_history`(既有行为)。落盘若发生在 `/clear` 后,快照 = 空 transcript + 满 history,恢复后 UI 空白但 LLM 仍有上下文。
- **选**:本 change 持久化**如实快照**当时状态(恢复 = 退出时所见);`/clear` 的 transcript/history desync 是**独立既有问题**,不在本 change 范围。design 注明该边界。
- **弃**:顺带修 `/clear` 同时清 history(超范围)、`/clear` 重置会话文件(额外接线)。
- **审查来源**:agent3(stall 前确认 line 1202——号对、文件应为 `app.rs` 非 `mod.rs`,第二轮 L-1 纠正)。

## 数据格式

`sessions/2f9c…e1.jsonl`(每行一个 `SessionLine`,`serde_json` 外部标签):
```
{"Meta":{"id":"2f9c…e1","provider":"anthropic","model":"claude-…","created_at":"2026-07-04T14:30:22Z","cwd":"H:\\…\\mysteries","app_version":"1.1.0"}}
{"Msg":{"System":"You are Mysteries…"}}
{"Msg":{"User":"帮我看下这个函数"}}
{"Msg":{"Assistant":{"text":"我来看看","tool_calls":[{"id":"call_1","name":"read_file","arguments":{}}]}}}
{"Msg":{"ToolResult":{"call_id":"call_1","content":"…","is_error":false}}}
{"Block":{"User":"帮我看下这个函数"}}
{"Block":{"Tool":{"id":"call_1","name":"read_file","args":{},"readonly":true,"status":"Done","output":"…","truncated":false,"exit":null}}}
{"Block":{"Assistant":"这个函数…"}}
```
恰一 `Meta` 首行;其后 `Msg`(重建 `agent_history`)与 `Block`(重建 `transcript`)。加载按 tag 分派、不依赖行序;`Msg` 里的 `System` 行在 resume 时按 D8 被当前默认替换。

## 接缝(实现挂载点)

- **序列化**:`tui/app.rs` 四类型加 `derive`。
- **保存**:`run_tui` **async 事件循环的 `ui_rx.recv()` 臂**(tui/mod.rs:250-259)——`handle_agent_event`(**保持同步**)调用**前**按引用算 `is_terminal`,调用**后** `if is_terminal { let h = state.agent_history.lock().await; if store.write(&meta, &h, &state.transcript).is_err() { drop(h); state.transcript.push(Notice) } }`。`store`/`meta` 为 `run_tui` async 体局部持有。**不挂 `handle_agent_event` 函数体内**(同步 fn 不能 `.await`,见 D3 第二轮 HIGH-1)。
- **恢复**:`run_tui`(tui/mod.rs:64;history 初始化第 90 行)——`--resume` 时 `store.load(latest)` → `(meta, history, transcript)`;`history.first_mut()` 若为 `System` 换当前默认(D8,空守卫);`agent_history` 用之替代默认 seed(在 mod.rs:90、**spawn 前**);`state.transcript` 经 `pub transcript` 字段(app.rs:407)在 `state` 构造(114)后注入;**provider 还原在 spawn 后经 `input_tx` 注入 `SetProvider`**(D9,绕开 agent move / state 未建)。非 resume → `new_session_id` 建新会话。
- **CLI**:`main.rs` real_main(43-48)解析 `--resume`;`CliPaths` 加 `config_dir`(L5;构造点 **main.rs:76 + cli.rs:959 测试助手 `temp_cli_paths` 两处都要补**,否则 `cargo test` 编译红——第二轮 MEDIUM-3),`run_tui` 加 `resume` 参(L6)。
- **id 生命周期**:非 resume 启动 `Uuid::new_v4()`;resume 复用被加载会话 id(续写同文件)。

## 风险 / 权衡

- **全量重写 vs append**:长会话每轮重写整文件;量级可控,换双结构 + compact 的实现简洁。未来需事件溯源再演进。
- **transcript / history 一致性**:两者在同一终止事件时点**一并捕获**(时序原子,非 FS 崩溃原子替换、亦非内容逐字一致);**compact 后二者合法发散**(history 收缩为 summary 入 LLM、transcript 保留完整滚动区仅显示),恢复后「UI 显示全 + LLM 上下文省」正是 compact 本意,非缺陷。旧 `ToolCard.id` 可能指向已被 compact 移除的 `call_id`——因 transcript 不入 LLM,无运行期隐患。
- **`/clear` desync**:既有,本轮如实快照(D10)。
- **落盘阻塞**:每轮一次全量写;若用同步 `fs::write` 在 async 上下文,单文件小、频率低(每轮一次)可接受;必要时 `spawn_blocking`。实现时以「不阻塞对话轮」为准。
- **`latest()` mtime**:Windows 下同秒可能并列;单测**不依赖 mtime 单调**,以写入顺序 + 显式间隔(或 `filetime` 设定)保证确定性。
