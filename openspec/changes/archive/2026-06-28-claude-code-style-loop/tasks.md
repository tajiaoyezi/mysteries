# Tasks — claude-code-style-loop

> TDD 分界:**强制红-绿** = headless 纯逻辑(A 收尾 / B 中断信令+select+Esc 三态 / C 合并+msgs);**事后 insta** = 渲染(Interrupted notice / 时间线 / 欢迎居中);**已实现补测** = D 八项(只确认绿 + spec↔test 对齐,不回退不重写);**手动** = tick / 终端循环 / TUI 冒烟。
> 🔴 三个**红灯停点**:① 强制收尾行为(2.2)② 中断并发路径(3.2)③ `TranscriptBlock::Tool` 变体(4.1)—— 各在测试首次成型、贴出红灯输出后**停下等确认**,再写绿。

## 1. A — 安全网值(config)

- [x] 1.1 `src/config/mod.rs`:`DEFAULT_MAX_ITERATIONS` 8 → 50。
- [x] 1.2 测:`resolve` 未设 `max_iterations` 时取 50(既有 config-layering 默认常量测随之更新,值不在 spec 钉死)。

## 2. A — 跑到结束 + 强制收尾(agent-loop,强制 TDD)

- [x] 2.1 【红】先只写测,运行确认失败(原因正确非编译错):MockProvider 脚本「连续 N 轮 tool_call」+ `max_iterations = N`,断言**第 N+1 次** `provider.complete` 的 `ModelRequest.tools` 为空、其文本经 `Ok(text)` 返回且入 history。
- [x] 2.2 🔴 **红灯停点①**:贴出 2.1 测试代码 + 失败输出,**停下等确认**(触顶强制收尾这条新行为路径首次成型)。
- [x] 2.3 【绿】最小实现:循环跑满 `max_iterations` 仍未自然终止时,追加一次 `provider.complete`(`tools` 传空);有文字 → `Ok(text)`,仍空 → `AgentError::MaxIterations`。不提前加未被测试要求的功能。
- [x] 2.4 边界(连写不停):强制收尾仍无文字 → `MaxIterations`;强制收尾那次 provider `Err` → `AgentError::Provider`(既有「provider 错误致命」分流复用)。
- [x] 2.5 零回归:既有 agent-loop 测试(自然终止 / 多轮编排 / observer / set_model)保持绿;`run` 仍委托 `run_observed`。

## 3. B — 运行中可中断(tui,新并发路径,强制 TDD)

- [x] 3.1 `src/tui/channel.rs`:加 `UserInput::Interrupt` 与 `AgentEvent::Interrupted`;设计**独立**中断通道(不复用 `input_rx`)。
- [x] 3.2 🔴 **红灯停点②**:先只写测并贴红灯输出,**停下等确认**(中断并发路径首次成型):MockProvider 在 `complete` 中挂起,投 `Prompt` 后投 `Interrupt`,断言本轮以 `Interrupted` 收场、状态回 `Idle`、**provider 未被再次调用**、agent task 存活。
- [x] 3.3 【绿】`run_agent_task` 的 `Prompt` 分支用 `tokio::select!`(本轮 `run_observed` vs 中断信号);中断到达 drop run future、发 `Interrupted`、回 `Idle`、程序不退。
- [x] 3.4 测:中断不消费 `input_rx` 中排队的 `Prompt`(独立通道验证)。
- [x] 3.5 Esc 三态(红-绿):`on_key` / `should_exit` 按「pending=拒绝 / 运行中=投 Interrupt / 就绪=退出」分流,且仅 `KeyEventKind::Press`。
- [x] 3.6 事后 insta(对眼):`Interrupted` 落 `⊘ 已中断本轮` notice 块,midnight / daylight 带色快照各一帧,人工对 `设计规范/03`(notice / `info.fg`,非致命)审核后锁定。

## 4. C — 单一时间线 transcript(tui,数据模型变体,强制 TDD)

- [x] 4.1 🔴 **红灯停点③**:先只写测并贴红灯输出,**停下等确认**(`TranscriptBlock::Tool` 变体 + 合并逻辑首次成型):`apply(ToolCallStarted{id})` → transcript 到达位置出现 `running` 的 `Tool` 块;`apply(ToolCallFinished{id})` → 按 `id` 回填 done/error + output/exit;断言无独立 `tool_cards` Vec。
- [x] 4.2 【绿】`TranscriptBlock` 加 `Tool(ToolCard)`;`AppState` 删 `tool_cards` Vec,改 push/backfill;`ToolCallFinished` 无匹配 `id` 安全降级(忽略、不 panic)测。
- [x] 4.3 `render`(红-绿可测部分):顺序遍历 `transcript` 渲 `Tool` 块(`设计规范/03` C5),移除「末尾汇总工具卡」路径;`msgs` 改为只计 `User`/`Assistant` 块(不含 `Tool` / 命令块)。
- [x] 4.4 事后 insta(对眼 / 回归):「`User` → `Tool`(done) → `Assistant`(最终回答)」时间线带色快照(最终回答为末块、不被工具卡盖住);迁移 / 更新受影响的既有快照(`tool_cards` 相关)。

## 5. D — 据实纳入(工作树已实现、测试绿,只确认 + 对齐,不回退不重写)

- [x] 5.1 D① `DEFAULT_SYSTEM_PROMPT` 身份约束 + 单测:确认绿,spec 短语(`Do not claim to be Claude` 等)↔ 单测断言一致。
- [x] 5.2 D② 按键去重(仅 `Press`):确认 `on_key` / `should_exit` / 滚动键既有去重测绿。
- [x] 5.3 D③–⑦ 排版/度量:文本换行 + 悬挂缩进、`◆`/`>` marker、`display_width`(emoji/零宽)、输入框光标定位、欢迎屏居中 + 垂直留白 —— 确认既有断言 / 带色快照绿。
- [x] 5.4 D⑧ 滚动:`scroll_up`/`scroll_down`(行级)、鼠标滚轮(`handle_scroll_mouse`)、终端 guard 鼠标捕获 —— 确认既有逻辑测 + 快照绿。

## 6. 收尾验证

- [ ] 6.1 `cargo build` 通过;`cargo test` 全绿(含新红-绿与迁移后的 insta)。
- [ ] 6.2 `openspec validate claude-code-style-loop --strict` 通过。
- [ ] 6.3 TUI 手动冒烟(非自动):Esc 三态(等授权拒绝 / 运行中中断回 Idle 不退 / 就绪退出);一轮跑到自然结束后最终回答钉底可见;PageUp/PageDown + 鼠标滚轮滚动正常。
