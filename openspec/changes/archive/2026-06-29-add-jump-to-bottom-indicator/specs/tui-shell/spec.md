## ADDED Requirements

### Requirement: 跳到底部提示与新消息计数

当 transcript **未跟随底部**(用户经 `PageUp` / `Home` 等滚离底部)时,系统 SHALL 在 transcript 视口底部、输入框上方钉一条单行提示 pill;**跟随底部时 pill MUST 隐藏**。pill 文案两态:自滚离底部以来无新增助手消息时为 `跳到底部 (ctrl+End) ↓`;新增了 N(≥1)条**已完成的助手消息**时为 `N 条新消息 (ctrl+End) ↓`。新消息计数 SHALL 只计助手消息(一轮答复 = 1),MUST NOT 计 user 回显 / 工具卡 / notice;计数在**未跟随底部**期间累加、回到底部跟随时 MUST 清零。`Ctrl+End` SHALL 使 transcript 回底并恢复跟随(`End` 亦可),回底后 pill 隐藏、计数清零。pill 渲染 SHALL 局部覆盖(仅 pill 宽,不留全宽黑带),配色用 theme token(adapt 设计规范 C14),含 `↓` glyph。计数增量逻辑 SHALL 为纯函数、可单测。

#### Scenario: 跟随底部时无 pill

- **WHEN** transcript 处于跟随底部态
- **THEN** 不渲染跳到底部 pill,新消息计数为 0

#### Scenario: 滚离底部显示「跳到底部」

- **WHEN** 用户经 `PageUp` 滚离底部,其间无新增助手消息
- **THEN** 视口底部渲染 `跳到底部 (ctrl+End) ↓`

#### Scenario: 滚离底部期间新助手消息累加(仅助手)

- **WHEN** 已滚离底部,模型完成 1 条助手回复(其间含若干工具卡)
- **THEN** pill 显示 `1 条新消息 (ctrl+End) ↓`(工具卡 / user 回显不计)
- **WHEN** 再完成 1 条助手回复
- **THEN** pill 显示 `2 条新消息 (ctrl+End) ↓`

#### Scenario: Ctrl+End 回底清零

- **WHEN** pill 显示 `2 条新消息 (ctrl+End) ↓`,按 `Ctrl+End`
- **THEN** transcript 回底并恢复跟随,pill 隐藏,计数清零
