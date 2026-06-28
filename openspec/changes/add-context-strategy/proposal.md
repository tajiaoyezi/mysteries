## Why

技术方案 §13 把 1.1 Token 压缩的扩展缝定为 `ContextStrategy`(Loop 构造请求前过一遍),并称「1.0 已就位直通+截断、1.1 换实现」。但代码核实:**该缝不存在**——`src/agent/mod.rs` 每轮直接 `messages: history.clone()` 请求,无 `ContextStrategy` 类型、无 `context.rs`(agent 目录仅 `mod.rs` + `message.rs`)。

本 change 据实把这道缝建出来:引入 `ContextStrategy` trait + 默认 `Passthrough`(原样直通,行为与现状逐字节等价)+ Loop 接线,为 1.1 压缩(`add-token-compaction`)预留**可替换的注入点**。本 change **只建缝、不压缩**(Passthrough 不改变任何消息),不依赖 `expose-token-usage`,可与其并行。

## What Changes

- **新 capability `context-strategy`**:`ContextStrategy` trait(async、`Send + Sync`、dyn 安全),方法 `prepare(&self, history: &[Message]) -> Result<Vec<Message>, ContextError>`——在每轮请求前由 history 产出「实际发送给 provider 的 messages」。
- **`Passthrough` 默认实现**:`prepare` 原样克隆返回 history,行为与「直接用 history 请求」逐字节一致。
- **Agent 接线**:`Agent` 持 `Box<dyn ContextStrategy>`(默认 `Passthrough`);`run` / `run_observed` 每轮请求前改为 `let msgs = strategy.prepare(&history).await?`,以 `msgs` 构造 `ModelRequest`;提供注入入口供后续压缩接入。既有循环 / 终止 / 错误 / 事件契约**零回归**。
- **`ContextError`**:最小错误类型(本 change Passthrough 恒 `Ok`;为压缩实现预留 Result 通道)。

## Capabilities

### New Capabilities
- `context-strategy`: 请求前上下文处理钩子,默认 `Passthrough`(原样直通);trait async / dyn 安全,为 1.1 压缩预留注入点。

### Modified Capabilities
<!-- 无:agent-loop 既有 requirement 因 Passthrough 等价而不变,以零回归测试锁定(见 design ②)。 -->

## Impact

- **code**:新增 `src/agent/context.rs`(`ContextStrategy` trait + `Passthrough` + `ContextError`);`src/agent/mod.rs`(`Agent` 加 `strategy` 字段 + 每轮 `prepare` 接线 + `mod context` + 注入入口)。
- **并行说明(与 `expose-token-usage` 同期)**:本 change 改 `src/agent/mod.rs` 的 **loop 主体 / struct / 新增测试**;`expose-token-usage` 改同文件**测试模块**里的 `ModelResponse` 构造点(补 `usage`)。两组改动不相邻、逻辑独立,git 多半自动合并,余下由主 agent 收口。本 change **不新增 `ModelResponse` 构造点**(Passthrough 走纯 `Vec<Message>` 单元测试 + 既有回归测试覆盖接线零回归),以最小化交叠。
- **provider / tui / config 不受影响**:钩子全在 agent 层;不碰 `ModelResponse`、不引入 token 计量。
- **deps**:零新增(`async-trait` 已在,Provider trait 已用)。
