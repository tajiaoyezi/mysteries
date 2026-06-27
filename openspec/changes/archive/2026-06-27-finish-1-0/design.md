## Context

Anthropic 落地后(全链 archived),§12 step5 仅剩三块:内置命令(§8)、超时接线(`config.timeout_secs` 未被用)、`ToolOutcome.exit`(C5 foot 数据源)。用户已定**单 change**收掉,1.0 feature-complete。本 change 不开 1.x(§13)。

现状(real code):`run_agent_task(agent, input_rx, ui_tx, ctx)` 顺序 `recv` → `Prompt` 重建 `[System,User]` → `run_observed`;`Agent{ model: String(私) }`;`AppState{ transcript: Vec<TranscriptBlock{User,Assistant,Error}>, tool_cards: Vec<ToolCard{..no exit}>, input, phase, pending_permission, scroll_offset, spinner_frame }`;`on_key` 提交走 `input_tx`;`ToolOutcome{content,is_error,truncated}`;`run_shell` 已算 `output.status.code()`;`OpenAiProvider::with_retry_policy` 私有(30s 默认),Anthropic 同。`StatusChanged(CallingModel)` 每轮迭代发一次。视觉权威:theme.rs/insta > `设计规范/02·03`(C5/C8/C9/C10) > 原型 > 推断。

## Goals / Non-Goals

**Goals:** 7 内置命令(解析 + 执行 + C8/C9/notice 渲染)、状态行 meta、/model 运行时切换、`config.timeout_secs` 接进 provider、`ToolOutcome.exit` + run_shell + C5 foot。全离线 red-green + 带色 insta + 3 帧对眼;既有行为零回归。

**Non-Goals:** 1.x 路线(token/持久化/PolicyEngine/MCP/subagent)、256/16 降级、运行时主题切换。

## Decisions

- **D1 capability 划分(6 delta 全 ADDED)。** NEW `builtin-commands`(解析 + 执行 + /model);MODIFIED `tui-shell`(C8/C9/notice 渲染 + 状态行 meta + C5 exit foot + `UserInput::SetModel`)、`agent-loop`(`set_model`)、`tool-system`(`ToolOutcome.exit`)、`builtin-tools`(run_shell exit)、`cli-runtime`(超时注入)。**全 ADDED**:均新关注点 / 加性,既有 requirement 文字不改、既有行为零回归。`provider-abstraction` 不动;openai/anthropic-transport **无 spec 变更**(超时仍 per-attempt,值来源变 = 构造器注入,impl 细节)。备选:命令 MODIFY tui-shell(弃:slash 命令是可独立命名的能力,NEW 更清晰)。

- **D2 命令解析 / 执行分离。** `parse_command(&str) -> Option<Command>`(纯,red-green)与执行(`AppState` 状态变更 / 发 `UserInput`)分开。`on_key` 提交:`parse_command` → `Some(cmd)` 执行命令、`None` 当 prompt 走 `input_tx`。`/clear` 清 transcript;`/help`·`/status` 追加块;`/exit` 置退出信号(`run_tui` 据此 break,或复用 `should_exit`);`/login`·`/logout`·`Unknown` → notice。

- **D3 /status 数据源,零 agent 改动。** `AppState` 持 session 快照(`provider`/`model`/`max_iterations`/`cwd`/`tools=7`,`run_tui` 装配时注入;`model` 可变,/model 切换同步);`msgs` = transcript 块数;**`iter` = UI 计当前轮 `StatusChanged(CallingModel)` 次数**(`apply` 在 `CallingModel` 自增,新 `Prompt`/`TurnComplete` 重置)。C9 块与状态行 meta 共用此快照。

- **D4 /model 全切换(动 agent-loop · 停点)。** `/model <name>` → `AppState` 乐观更新 model + 经 `UserInput::SetModel(name)` 发 agent-task;`run_agent_task` 的 `recv` 循环加 `SetModel` arm → 对 idle `agent`(绑定改 `mut`)调 `Agent::set_model`(**下一轮生效**,进行中轮不受影响,因循环顺序消费)。`/model` 无参 → notice 显当前。**capability:agent-loop +`set_model`、tui-shell +`SetModel`**,既有 `run`/`run_observed` 零回归。**红灯停点**于 `set_model` 接口。备选 not-yet(仅显示)弃(§8 列「切换」+ 用户预批停点)。

- **D5 超时注入。** openai/anthropic 暴露 timeout-taking pub 构造器(如 `with_attempt_timeout(base_url, creds, Duration)` 或公开 `with_retry_policy`),`select_provider` 据 `config.timeout_secs` 构造 `RetryPolicy{attempt_timeout: from_secs(secs), max_retries: 默认, backoff: 默认}` 注入。**openai/anthropic-transport 行为不变**(per-attempt timeout 依旧)→ 无 spec 变更;MODIFIED 落 cli-runtime。red-green:断言所选 provider 的 `RetryPolicy.attempt_timeout`(需 provider 暴露读取或构造器可验)。

- **D6 `ToolOutcome.exit` behavior-preserving。** `exit: Option<i32>`;`run_shell` 设 `output.status.code()`(本就 `Option<i32>`,**保留 content 里的 `exit:` 文本不动** → run_shell content 零回归);其余工具字面量机械 += `exit: None`。`ToolCard` +`exit`(从 `ToolCallFinished.outcome.exit` 填);C5 foot **仅 `Some` 渲**(`None` 不渲 → 既有工具卡快照零回归)。**C 作最后一组 task**(加结构字段牵动最广,殿后降风险)。

- **D7 测试分界。** 命令解析 / 执行状态(/clear、/status 块、/model 逻辑、notice)/ iter 计数 / `set_model` / 超时注入 / `ToolOutcome.exit` / run_shell exit = **red-green**;C8 / C9 / 状态行 meta / run_shell 卡 exit foot 渲染 = **带色 insta**;C8 / C9 / exit-foot 三帧 = **themed 对眼**;/exit 终端拆除 / live = **手动**。

## Risks / Trade-offs

- **[加 `ToolOutcome.exit` 牵动所有字面量]** 编译错遍及 tests/工具/agent/app → 缓解:机械 += `exit: None`,behavior-preserving;既有相等断言更新但语义不变;`cargo test` 全绿即验零回归。C 殿后。
- **[再动 archived agent-loop(set_model)]** → 缓解:additive 新方法,既有 `run`/`run_observed`/测试零触动;**红灯停点** + 既有 agent-loop 测试保持绿为闸。
- **[/model 切换时序]** 切换在进行中轮之后才生效 → 缓解:D4 明确「下一轮生效」+ UI 乐观显示,语义文档化;符合 `run_agent_task` 顺序消费模型。
- **[状态行 meta / iter 与既有 status 快照]** → 缓解:iter 纯 UI 派生(CallingModel 计数),不引 agent 改动;既有 phase 渲染不变,meta 为右侧加法。

## Migration Plan

`tui/*` 加命令解析/执行 + 块渲染 + 状态 meta + exit foot + SetModel + session 快照;`agent` +`set_model`;`tool` +`exit`(机械字面量更新);`provider` +pub timeout 构造器;`app::select_provider` 注入超时。回滚 = revert 本 change。无数据迁移。

## Open Questions

- `/exit` 退出与既有 `should_exit`(Esc / Ctrl-C)如何统一(共用退出信号路径)—— 实现期定。
- 重试次数是否随 config 可配(本 change 留默认常量,仅超时配)—— 留后续 / 顺带。
- run_shell content 里的 `exit:` 文本是否随结构化 exit 移除(本 change **保留**以零回归 content)—— 后续如需精简单列。
