## ADDED Requirements

### Requirement: 会话 history 跨轮累积

TUI agent task SHALL 维护跨 prompt 累积的会话 history(`System` + 历轮 `User`/`Assistant`/工具消息),**MUST NOT** 每投入一个 `UserInput::Prompt` 就从仅含 `System` 的空 history 重建。每轮 prompt 在既有 history 末尾追加当前 `User`,跑完 `Agent.run` 后将 working history 写回共享状态;下一轮 provider 请求 MUST 携带此前各轮消息。`/compact` 作用于该共享会话 history(与自动压缩同一 `Compacting` 逻辑)。

#### Scenario: 两轮后第二轮请求含第一轮消息

- **WHEN** 以 Mock provider(脚本:两轮各返回一段文本)驱动 agent task,连续投入两个 `UserInput::Prompt`
- **THEN** 第二轮发给 provider 的 messages 含第一轮的 `User` 与 `Assistant` 原文,共享会话 history 亦保留两轮完整记录;全程无终端
