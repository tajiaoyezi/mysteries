# add-session-persistence

## Why

会话历史与界面(`agent_history: Vec<Message>` + `transcript: Vec<TranscriptBlock>`)当前仅存内存,进程退出即丢失——无法跨会话续跑、无法回看既往对话与工具结果。对标 Claude Code(会话存盘、`--resume` 恢复、含会话元数据),补齐 1.x 路线图 §13 的 **1.2 持久化(`SessionStore`)**。

现状已具备干净接缝:
- `Message` 已 `derive(Serialize, Deserialize)`;`TranscriptBlock` / `ToolCard` 等 UI 类型加 `derive` 即可序列化(纯派生,行为零变)。
- 历史由调用方持有;`AppState::with_session_and_history(session, agent_history)` 构造器本就是外部注入历史的入口(恢复接缝已存在)。
- 保存 hook 落在**事件循环终止事件**(`handle_agent_event` 的 async 调用点,此处 `transcript` 与 `history` 同时可见);恢复 hook 落在 `run_tui` 初始化处。`run_agent_task` 是独立 spawn、拿不到未共享的 `transcript`,不作保存点。

## What Changes

**完整会话持久化(对标 Claude Code 保真度)**:每会话落盘为一个快照文件,含会话元数据 + 对话历史 + UI transcript;TUI 每轮结束全量重写;`--resume` 启动加载最近会话,完整还原对话、工具卡视觉、并以原 provider/model 续跑。

1. **`SessionStore`(headless 核心,强制 TDD)**:`uuid` session id、路径解析、快照保存(全量重写)、加载、最近会话查找。纯逻辑 + IO,Mock 临时目录可驱动。
2. **序列化扩展**:`TranscriptBlock` / `ToolCard` / `ToolCardStatus` / `StatusSnapshot` 加 `Serialize` / `Deserialize`(纯 `derive`,`PartialEq` 等既有派生不动,行为零变)。
3. **`SessionMeta`**:`{ id, provider, model, created_at, cwd, app_version }`。
4. **TUI 接线(IO 胶水,事后回归)**:`run_tui` 若 `--resume` 则加载快照,注入 `agent_history`(`System` 换当前默认)+ `transcript`、仿 `apply_set_provider` 还原 provider/model(缺失回退默认 + `Notice`);保存挂**事件循环 `handle_agent_event` 的终止事件**(`TurnComplete` / `CompactDone` / `Interrupted` / `Error`)——此处 `transcript` 与 `agent_history` 同时可见且一致,全量重写快照。
5. **CLI**:`--resume` flag(无参 = 最近会话)。

## Impact

- 新 capability:`session-persistence`
- **新依赖:`uuid`**(v4;用户要求对标 Claude Code 的 uuid 会话标识、全局唯一;`serde_json` 已在依赖树)
- Affected code:新 `src/session/`(`SessionStore` + `SessionMeta` + 快照读写);`tui/app.rs`(`TranscriptBlock` / `ToolCard` / `ToolCardStatus` / `StatusSnapshot` 加 `derive`);`tui/mod.rs`(事件循环终止事件落盘 hook + 恢复注入 + provider 还原仿 `apply_set_provider`);`main.rs` + `cli.rs`(`--resume` 解析、`CliPaths` 加 `config_dir`)
- 存储:每会话一个 jsonl 行式快照(`<config_dir>/sessions/<uuid>.jsonl`,每行一个 tagged record:恰一 `Meta` + 逐条 `Msg` + 逐块 `Block`),每轮**全量重写**(非 append 事件流,见 design D2)
- 回退:无 `--resume` 时**对话流程**与现状一致(新增副作用:建新会话文件、每轮快照写;写失败仅推 `Notice`、不阻断对话与 `agent_history`)
