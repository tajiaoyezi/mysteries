# 2026-06-27 · 09 · archive add-tui-shell

## 决策

- **TUI 骨架第一刀(cut1)**(§12 step4):引入 §3 双-task + mpsc/oneshot,套 ratatui 最小四区外壳,复用 `app` 装配,跑通一轮 | 主导:用户拍板 step4 先做骨架 | 依据:§3 / §8 / §12 / `设计规范/`
- **cut1/cut2 拆分**:cut1=骨架+最小外壳(本 change);cut2 `add-tui-rich`=结构化事件(**MODIFY agent-loop**)+工具卡 C5+全 phase+全主题+滚动+C6 diff body+C7 框 | 弃:一把梭整个 TUI
- **cut1 零 modified capability、不碰 agent-loop(关键)**:文本走 `DeltaSink→ChannelSink`、权限走 `PermissionDecider→ChannelDecider`(oneshot)| 依据:D2/D4;代价(工具仅权限时露面)留 cut2
- **D2 channel 协议 = §3 子集**:`AgentEvent{TextDelta/PermissionRequired/TurnComplete/Error}` / `UserInput::Prompt` / `PermissionRequest{tool_name,args,responder}`;不 derive `Clone`(单 UI task 消费 + 携 oneshot)
- **D3 `ChannelSink` 用 `mpsc::UnboundedSender`** | 契合 sync `on_text`;弃 bounded(async send 会逼 `on_text` 阻塞/丢字)
- **D4 `ChannelDecider` 跑通 §3 挂起-恢复、零 agent-loop 改动**:oneshot + `rx.await`,断开→`Deny`(fail-safe)
- **D5 `run_agent_task` 与终端解耦**:无终端、Mock 驱动可离线测一轮(含权限往返);`run_tui` 仅在其外包终端 + `select!`(手动冒烟)
- **D6 `TerminalGuard` RAII + panic hook(`Once`)**:panic 与正常退出都恢复终端(§12 要求)
- **D7 `render` 纯函数 → `TestBackend` + insta 事后快照**,首帧人工对眼 `原型截图/midnight-01`(README+config.yaml 强制人类关卡),非 red-green
- **D8 main 分流不引 clap**:`env::args` 查 `--headless`→`cli::run_cli`,否则 `run_tui`;`cli` 保留(headless+e2e,实证 `app` 双前端复用,验证 cli-assembly D3 划界)
- **D9 cut1 视觉结构态**(最小色)、粗 phase(就绪/忙/等待授权);全 `01-设计令牌` 主题 / diff body / 状态行右侧快照留 cut2;`设计规范/README` 强制的港·适配·丢弃映射 + 显式偏离已记
- **D10 deps**:`ratatui 0.29` / `crossterm 0.28`(event-stream)/ `insta 1`(dev);`tokio` += `sync`;不引 clap
- **两个停点(强制)**:§4.1 `ChannelDecider` 新权限流红灯停点(挂起-恢复 + fail-safe 语义,主 agent 审通过);§7.3 首帧 insta 对眼停点
- **审查修正**:① 首帧对眼**打回**——首版 C2 英文占位 + 漏 C6 动作行 → 补契约中文(✦ MYSTERIES / 标语 / 4 建议行)+ C6 `[y·允许][n·拒绝]`+Enter·Esc 提示 + C11 占位,重渲对眼通过后锁定 | 主导:主 agent 对眼;② `clippy::await_holding_lock`(`tui/mod.rs` 测试 `MutexGuard` 跨 `handle.await`)→ 调整断言顺序(先 await 后取 guard),clippy 维持零警告 | 主导:主 agent 审查
- **里程碑**:`cargo run` 进 TUI(一轮 输入→响应 + oneshot 权限框)、`--headless` 走 CLI;§3 双-task 架构落地

## 变更

- 新增 `src/tui/{mod,channel,app,render,terminal}.rs` + 2 insta 快照;`lib.rs` += `pub mod tui;`;`main.rs` += `--headless` 分流
- 验证:`cargo test` 106 passed / 1 ignored(含 2 insta 快照测 + 各离线编排测);`clippy` 零警告;`fmt` 通过
- 新依赖:`ratatui 0.29` / `crossterm 0.28`(event-stream)/ `insta 1`(dev)+ `tokio` sync feature(`Cargo.lock` += 传递依赖)
- archive:`changes/add-tui-shell` → `changes/archive/2026-06-27-add-tui-shell`;`specs/` 新增 `tui-shell`(7 requirements)

## 待决

- **cut2 `add-tui-rich`**:结构化 `AgentEvent`(`ToolCallStarted/Finished` / `StatusChanged`,**MODIFY agent-loop**)+ 工具卡 C5 + 全 phase C10 + 全 `01-设计令牌` 主题(`theme.rs` + token 单测,Midnight/Daylight)+ C6 diff body + C7 致命框 + 滚动 + insta 全锁
- 内置命令(C8/C9,`/help` 等)、Anthropic、`tool_mode` 降级、step5 收尾(流式/超时/重试打磨)
- `run_tui` select! 循环 + 终端仅手动冒烟(编排逻辑已离线测);输入框多行 / 历史 / 滚动键位 cut1 最小,留体验 change
- 状态行右侧快照(provider/model/iter/cwd)+ `◇/▲` glyph 留 cut2

## 引用

- change:`add-tui-shell`(rationale / rejected alternatives 全量见 design.md D1–D10;archive 路径 `changes/archive/2026-06-27-add-tui-shell`)
- 技术方案 §3 / §4 / §8 / §12 step4
- `设计规范/02-布局与交互`(四区 + D1 顶栏 + D2 状态行)、`03-组件清单`(C1/C2/C3/C4/C6/C10/C11)、`原型截图/midnight-01-欢迎态.png`(首帧对眼基准)
- 前置 change:`add-cli-assembly`(决策记录 08)
- session log:无专属 checkpoint —— 子 agent propose + implement(两停点:§4.1 权限流、§7.3 对眼);主 agent review(核 API / §3 架构 / 测试分界、§7.3 对眼打回英文占位并令补契约文案、抓 `await_holding_lock`)+ commit / archive
