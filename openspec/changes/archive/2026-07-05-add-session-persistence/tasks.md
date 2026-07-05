# Tasks — add-session-persistence

红灯纪律:红灯独立成步,以断言失败落红(非编译错)——新类型/新签名允许红灯内先落桩(签名成型、空产出/旧语义);为既有类型加 `derive` 属编译红豁免(先写 round-trip 测试、后加 derive)。**红灯停点**:2.1 为 `SessionStore` 新接口首次成型,测试 + 失败输出贴出后**停下等确认**再进绿灯;1.x / 3.x / 4.x 可连写。
执行 agent MUST NOT:git 写操作、修改既有快照/夹具以过测、勾选第 5 节真机任务、`cargo fmt` 全仓、kill 用户进程。

## 1. 序列化扩展(TDD;derive 编译红豁免)

- [x] 1.1 红:round-trip 单测——`TranscriptBlock` 各变体(`User`/`Assistant`/`Tool`/`Error`/`Help`/`Status`/`Notice`)、`ToolCard`(含 `args: Value`、`exit: Some`/`None`、`status` 三态)、`ToolCardStatus`、`StatusSnapshot` 序列化→反序列化 `== 原值`。桩 = 给这些类型加 `Serialize`/`Deserialize` derive。运行确认(derive 前编译红豁免)
- [x] 1.2 绿:加 `derive`,round-trip 全绿;既有 `Clone`/`Debug`/`PartialEq`/`Eq` 派生不动、行为零回归(全量测试保绿)

## 2. session 核心:SessionMeta / SessionLine / SessionStore(强制 TDD)

- [x] 2.1 红(**停点**):落位以下,桩为空/占位实现,断言红:
  - `SessionMeta { id: String, provider: String, model: String, created_at: String, cwd: PathBuf, app_version: String }`(Serialize/Deserialize)
  - `SessionLine` enum:`Meta(SessionMeta)` / `Msg(Message)` / `Block(TranscriptBlock)`(Serialize/Deserialize,外部标签)
  - `SessionStore::new(root: PathBuf)`;`new_session_id() -> String`(uuid v4 桩恒定值);`session_path(&self, id) -> PathBuf`;`write(&self, meta, history: &[Message], transcript: &[TranscriptBlock]) -> io::Result<()>`(桩 no-op);`load(&self, id) -> io::Result<(SessionMeta, Vec<Message>, Vec<TranscriptBlock>)>`(桩 Err);`latest(&self) -> io::Result<Option<String>>`(桩 None)
  - 测试(断言红):
    - `SessionLine` tag 格式:`Meta`/`Msg`/`Block` 三 variant 各序列化为 `{"Meta":…}` 等(锁 jsonl 行契约)
    - `write` → `load` round-trip:`meta` + `history`(含 System/User/Assistant.tool_calls/ToolResult)+ `transcript`(含 Tool 卡 exit/truncated)完整一致
    - 分派不依赖行序:构造 `Meta`/`Block`/`Msg` 交错的 jsonl → `load` 仍正确归位三容器(边界锁定)
    - `latest`:空目录 → `None`;多文件 → mtime 最新 id(测试以**写入顺序 + 显式间隔或 `filetime` 设定**保证确定性,不依赖 mtime 单调——审查 L4);非 `.jsonl` 文件忽略
    - `new_session_id`:实现后为合法 uuid **且两次调用互不相等**(2.2 换真实现时补;红灯桩仅锁签名——审查 L2 唯一性)
    - **`meta.id` 与文件名一致**(审查 M1):`write` 后 `session_path(meta.id)` 存在、`load` 回来的 `Meta.id == 文件名 id`
    - 损坏行(非法 JSON 行 / 未知 tag)→ `load` 策略:整体 `Err`(不静默吞)
    - **`Meta` 行约束**(审查 L2):零 `Meta` 行 或 多于一个 `Meta` 行 → `load` 返回 `Err`
    - **二次 write 同 id(compact 场景,审查 M1)**:先以 10 条 `history` write、再以 4 条 write → 文件仅含 4 条 `Msg`(旧 6 条不残留;锁 D2/D3 全量重写、对齐 spec「全量重写反映 compact」scenario)
    - 空 `history` 且空 `transcript`:仅 `Meta` 行,round-trip 得空两容器
- [x] 2.2 绿:最小实现——`uuid::Uuid::new_v4()`;`write` 全量重写(`Meta` 行 + `history` 逐条 `Msg` 行 + `transcript` 逐块 `Block` 行,每行 `serde_json::to_string`);`load` 逐行 parse+分派;`latest` 扫目录取 mtime 最新;`sessions/` 目录不存在时 `write` 先建
- [x] 2.3 红→绿:`replace_system_head(history: &mut Vec<Message>, prompt: &str)` 纯函数(**D8;审查 M-3 定为强制 TDD 半区**——纯 Vec 操作、IO 无关,不当 IO 胶水)——`history.first_mut()` 若为 `System` 则换 `prompt`;空 history 或首元非 `System` 时不改(空守卫,审查 LOW-5)。测试:①含旧 `System` → 首元变新 prompt、其后逐条不变;②空 history → 不 panic、仍空;③首元非 `System` → 不改

## 3. TUI / CLI 接线(IO 胶水,事后回归)

- [x] 3.1 落盘 hook(**审查修订 H1 → HIGH-1:挂 async 事件循环调用点、非 `handle_agent_event` 函数体**——后者是同步 `fn`、不能 `.await` 读 tokio-`Mutex` 的 `agent_history`):`run_tui` 的 `ui_rx.recv()` 臂(tui/mod.rs:250-259)——`handle_agent_event`(**保持同步、零测试面波及**)调用**前**按引用算 `is_terminal`(`TurnComplete`/`CompactDone`/`Interrupted`/`Error`),调用**后** `if is_terminal { let h = state.agent_history.lock().await; store.write(&meta, &h, &state.transcript) }` 全量重写(async 等待对 `CompactDone` 正确:等 run_agent_task 于 1153 `drop` guard、其间无 await 不死锁);写失败 → drop 锁后 `state` 推 `Notice`、**不阻断对话与 `agent_history`**(以现有事件循环测试保绿 + 加一条注入失败 store 的 Notice 断言)
- [x] 3.2 `run_tui` 恢复:`--resume` 时 `store.load(latest)` → 注入 `agent_history`(经 2.3 `replace_system_head` 换当前 `DEFAULT_SYSTEM_PROMPT`,D8)替代默认 seed(**spawn 前** mod.rs:90)+ `state.transcript`(state 构造 114 后注入);**provider 还原在 spawn 后经 `input_tx` 注入 `UserInput::SetProvider{id: meta.provider, model: meta.model}`**(审查修订 MEDIUM-2:run_tui 前段 `agent` 已 move@104、`state` 未建@114;复用 `run_agent_task` 现成 `SetProvider` 臂 mod.rs:1133-1145,其 profile/凭据缺失 `Err` **已自动 `send(Notice)`**@1143、回退零新测试面;**不用 `resolve_active_provider`**——静默 remap 不发 Notice);非 resume → `new_session_id` 建新会话
- [x] 3.3 `main.rs` real_main:解析并剥离 `--resume`(无参),经 `run_tui` 新增 `resume` 参传入;`--headless` 路径不受影响
- [x] 3.4 `CliPaths` 加 `config_dir` 字段(cli.rs 定义;**两处构造点都补:`default_paths` main.rs:76 + 测试助手 `temp_cli_paths` cli.rs:959**——审查 MEDIUM-3,漏 cli.rs:959 则 `cargo test` 编译红);store 根 = `config_dir.join("sessions")`

## 4. 门禁

- [x] 4.1 `cargo test --lib` 全绿;`cargo clippy --all-targets -- -D warnings` 零警告
- [x] 4.2 `openspec validate add-session-persistence --strict` 通过
- [x] 4.3 `cargo build` 通过(新增 `uuid` 依赖解析)

## 5. 真机核验(主 agent / 用户;执行 agent MUST NOT 勾)

- [x] 5.1 跑一轮多回合对话(含工具调用)→ 退出 → `mysteries --resume` → 对话历史 + 工具卡完整还原、以原 provider/model 续跑
- [ ] 5.2 落盘容错:sessions 目录只读/不可写 → 对话不中断、仅提示
- [ ] 5.3 compact 后 `--resume`:还原的是压缩后历史(不重复旧消息)
- [x] 5.4 无 `--resume` 冷启动:行为与现状一致;新会话文件生成于 `sessions/`
- [ ] 5.5 provider 缺失回退(审查 M-2):把会话 `meta.provider` 改成配置中不存在的 → `--resume` 期望回退默认 provider + `Notice`、不 panic、可续跑

> **收尾记录(2026-07-04,主 agent 依真机证据)**:5.1 核心闭环真机通过(用户验:deepseek 会话的对话 + 工具卡 + provider/model 完整还原);5.4 冷启落盘验证(`sessions/5a41be41….jsonl`,Meta 完整、`created_at` unix secs、`app_version=1.1.0`)。5.2/5.3/5.5 边界(容错 / compact / provider 缺失回退)未逐项真机,代码经两轮对抗审查 + 单测覆盖,评估低风险入观察池。fmt 污染(执行 agent 全仓 `cargo fmt` 重演)已 revert 4 文件。**已知后续(用户真机提出)**:退出/启动提示「resume 哪个会话」(Claude Code `--resume` 交互式会话列表)= design D6 留的后续 UX 增强;本轮 `--resume` 只做「续最近」(≈ claude `--continue`),交互式列表另开 change。
