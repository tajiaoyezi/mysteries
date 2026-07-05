# 2026-07-04 · 49 · archive add-session-persistence

## 决策
- 会话持久化 = jsonl 行式快照(`Meta` + `Msg` + `Block`)+ 每轮全量重写 + `--resume` 续最近 | 选:完整 UI 状态(`agent_history` + `transcript` + `SessionMeta`)、uuid、provider/model 元数据还原 | 弃:仅 `agent_history` 最小闭环(用户要完整保真)、append 事件溯源(双结构需统一事件模型 + replay 引擎、独立大重构)、SQLite(违内核自实现最简解) | 主导:用户拍板对标 Claude Code | 依据:code 接缝(`Message` 已 Serialize、`AppState::with_session_and_history` 注入口)
- 落盘挂 **async 事件循环调用点**(`ui_rx` 臂,非 `run_agent_task`、非同步 `handle_agent_event` 体) | 弃:`run_agent_task`(独立 spawn、拿不到未共享的 `transcript`)、`handle_agent_event` 函数体(同步 `fn` 不能 `.await` 读 tokio-`Mutex`)、`try_lock`(`CompactDone` 持锁发出会丢快照) | 主导:两轮对抗审查(第一轮 H1 → 第二轮 HIGH-1) | 依据:探针级 code 核实
- provider 还原经 **`SetProvider` channel**(spawn 后 `input_tx` 注入) | 弃:run_tui pre-spawn 仿 `apply_set_provider`(`agent` 已 move / `state` 未建)、复用 `resolve_active_provider`(静默 remap、不发 Notice) | 主导:第二轮审查 MEDIUM-2 | 依据:复用现成已测 `SetProvider` 臂 + 其 Notice 通道
- resume 时 System 换当前 `DEFAULT_SYSTEM_PROMPT`(D8,身份/工具契约应 fresh)、`/clear` transcript-history desync 另案(D10)、uuid v4、`created_at` 用 unix secs(不引 chrono) | 主导:审查(agent2 M4)+ 主 agent 定、可否决
- 交互式会话列表留后续 | 选:本轮 `--resume` 只做「续最近」(语义 ≈ claude `--continue`)| 弃(本轮):启动列出历史会话供选择(claude `--resume` 式,用户真机提出)| 主导:用户提出 + 主 agent 建议另开 change | 依据:design D6 最小闭环

## 变更
- 新 capability `session-persistence`(spec:快照落盘 / 加载还原 / 最近查找 / uuid 标识 / `--resume` / 落盘容错 / UI 类型序列化保真)
- 新 `src/session/`(`SessionStore` + `SessionMeta` + `SessionLine` + `replace_system_head` 纯函数)+ `uuid` v4 依赖(唯一新依赖)
- `tui/app.rs` 四类型(`TranscriptBlock`/`ToolCard`/`ToolCardStatus`/`StatusSnapshot`)加 `Serialize`/`Deserialize`;`tui/mod.rs` 事件循环落盘 hook + `prepare_session_startup` + `SetProvider` 注入 + `write_session_snapshot`(用 `state.session` 实时 prov/model);`main.rs`/`cli.rs` `--resume` 剥离 + `CliPaths.config_dir`(三处构造点)
- 测试 585 → 608;两轮对抗审查(第一轮 3 路初审 + 第二轮 2 路复审)各抓 1 个阻断 HIGH(均出「接缝可实现性」维、全修准解、复用已测通道零新测试面);执行 agent 全仓 `cargo fmt` 污染 4 无关文件 + 甩锅「既有改动」,主 agent `git checkout` revert
- 真机:5.1 resume 还原(deepseek 会话对话 + 工具卡 + provider/model 完整还原)+ 5.4 冷启落盘(Meta 完整、`created_at` unix secs)通过;5.2/5.3/5.5 边界入观察池

## 待决
- 交互式会话列表(启动列出历史会话 + 选择 resume)—— 下一个 change
- `--resume` 命名:当前语义 ≈ claude `--continue`;未来加列表时或加 `--continue` 区分二者
- 5.2/5.3/5.5 边界真机(落盘容错 / compact 后 resume / provider 缺失回退)—— 观察池

## 引用
- OpenSpec change:add-session-persistence
- 前置:[[2026-06-27-14-archive-finish-1-0]](§13 路线图 1.2 持久化 `SessionStore`)
- 过程:执行 agent fmt 污染重演,见 [[git-verify-shared-tree]] 记忆
