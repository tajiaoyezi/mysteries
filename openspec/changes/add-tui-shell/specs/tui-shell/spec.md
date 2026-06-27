## ADDED Requirements

### Requirement: §3 双-task + channel 协议

系统 SHALL 以技术方案 §3 的双-task 架构运行 TUI:一个 agent task(跑 `Agent.run`)与一个 UI task(渲染 + 事件)经 channel 通信。UI→Agent 用 `UserInput`(cut1 至少 `Prompt(String)`);Agent→UI 用 `AgentEvent`(cut1 子集:`TextDelta` / `PermissionRequired`(携 `oneshot::Sender<PermissionDecision>`)/ `TurnComplete` / `Error`)。`AgentEvent` MUST NOT 要求 `Clone`(被单一 UI task 独占消费,且 `PermissionRequired` 携不可 `Clone` 的 oneshot)。

#### Scenario: 一轮 prompt 经 channel 往返

- **WHEN** 以 Mock provider(脚本:一段文本回复)装配 agent task,向其投入 `UserInput::Prompt`
- **THEN** UI 端从 channel 依次收到 `TextDelta`(一或多段)与 `TurnComplete`,全程无需终端

### Requirement: ChannelSink 文本增量转发

`ChannelSink` SHALL impl 既有 `DeltaSink`,其 `on_text` MUST 把文本增量经 `mpsc::UnboundedSender<AgentEvent>` 以 `TextDelta` 推出(unbounded 同步 send,契合 sync `on_text`,不阻塞 agent task)。

#### Scenario: on_text 推出 TextDelta

- **WHEN** 对一个持 channel sender 的 `ChannelSink` 调用 `on_text("hello")`
- **THEN** channel 接收端收到 `AgentEvent::TextDelta("hello")`

### Requirement: ChannelDecider 权限 oneshot 往返

`ChannelDecider` SHALL impl 既有 async `PermissionDecider`:`decide` MUST 创建 `oneshot`、向 UI 发 `AgentEvent::PermissionRequired{tool_name, args, responder}`、在 `responder` 的 `rx.await` 处挂起,收到决策后返回之;若 UI 端 sender / responder 断开,MUST 返回 `PermissionDecision::Deny`(fail-safe)。本机制 MUST 不改动 `agent-loop`(经既有 `PermissionDecider` 缝接入)。

#### Scenario: 权限请求挂起-恢复

- **WHEN** `ChannelDecider::decide` 被调用,UI 收到 `PermissionRequired` 后经 `responder` 回送 `Allow`
- **THEN** `decide` 返回 `Allow`(挂起在 `rx.await`、收到后恢复)

#### Scenario: UI 断开 fail-safe 拒绝

- **WHEN** `decide` 发出请求后 UI 端 responder 被丢弃(`rx` 出错)
- **THEN** `decide` 返回 `Deny`,不 panic

### Requirement: agent-task 一轮编排(Mock 驱动 · 无终端)

系统 SHALL 提供可在**无终端**下以 Mock provider 驱动的 agent-task 编排:投入一个 prompt,经 `ChannelSink`(文本)与 `ChannelDecider`(权限)跑完一轮 `Agent.run`,把事件流回 channel。含 `RequiresConfirmation` 工具的脚本 MUST 能走通「`PermissionRequired` → 回送决策 → 继续 / 拒绝入 history」。

#### Scenario: 含权限的一轮编排

- **WHEN** Mock 脚本为「轮1 → 一个 RequiresConfirmation 工具的 tool_call、轮2 → 终复文本」,投入 prompt 并对 `PermissionRequired` 回送 `Allow`
- **THEN** channel 依次见到权限请求与文本事件,工具被执行,最终 `TurnComplete`;全程无终端、不触网

### Requirement: ratatui 四区最小外壳渲染

系统 SHALL 用 ratatui 渲染 `设计规范/02-布局与交互` 的四区布局:顶栏(C1,仅品牌 `✦ mysteries  agent · v1.0`)/ transcript(空会话 → C2 欢迎态;有会话 → user/assistant 文本块)/ 状态行(C10,cut1 粗 phase:就绪 / 忙 / 等待授权)/ 输入框(C11,`mysteries ▸ ` + 占位)。`PermissionRequired` pending 时,C6 权限框 MUST 内联钉在状态行上方。渲染 MUST 可经 `ratatui::backend::TestBackend` 快照验证(`insta`),首帧人工对 `原型截图/midnight-01-欢迎态.png` 审核。

#### Scenario: 欢迎态结构快照

- **WHEN** 空会话状态渲染到 `TestBackend`
- **THEN** 快照含 顶栏品牌行、C2 欢迎态(wordmark + 标语 + 建议行)、状态行、输入框占位四区结构,且与锁定的 `insta` 快照一致

#### Scenario: 权限态内联框

- **WHEN** 存在一个 pending 的 `PermissionRequired`(工具名 + args)时渲染
- **THEN** 快照在状态行上方含 C6 权限框(`▲ 需要授权` + 工具名/args + `[y·允许][n·拒绝]`),与锁定快照一致

### Requirement: 终端生命周期恢复

系统 SHALL 以 RAII guard 管理终端:进入时启用 raw mode + alternate screen,**正常退出与 panic 都 MUST 恢复**(离开 alternate screen、关 raw mode),避免把用户终端留在损坏态。

#### Scenario: 退出恢复终端

- **WHEN** TUI 正常退出或 agent / UI task panic
- **THEN** 终端被恢复(raw mode 关闭、回到主屏),不残留损坏态

### Requirement: main 分流(TUI 默认 / headless 回退)

`main` SHALL 默认进入 TUI;当传入 `--headless` 时 MUST 改走既有 `cli::run_cli`(headless 路径与其 e2e 测保留)。两路 MUST 复用 `app::{load_config, select_provider, assemble_agent}`(同一装配,不同前端)。

#### Scenario: headless 回退到 CLI

- **WHEN** 以 `--headless` 启动并给定 prompt
- **THEN** 走 `cli::run_cli`(stdout 流),不进入 ratatui

#### Scenario: 默认进入 TUI

- **WHEN** 不带 `--headless` 启动
- **THEN** 进入 ratatui TUI(四区外壳),prompt 由输入框交互获取
