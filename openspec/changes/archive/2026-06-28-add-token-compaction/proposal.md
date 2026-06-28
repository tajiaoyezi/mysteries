## Why

1.1 的两步地基已就位(`expose-token-usage` 的真实 `usage` + `add-context-strategy` 的 `ContextStrategy` 缝),但默认 `Passthrough` 不做任何压缩——长会话持续累积 history 直到撞 provider context window 上限(请求被拒 / 截断)。本 change 落地 **1.1 的正题:真正的上下文压缩**,贴近 claude code:接近 window 上限时调 provider 把历史压成**结构化 summary**,从 `system`(含 summary)+ 极少最近原文继续,使长会话可持续。含**自动**(按阈值)与**手动 `/compact`** 两条触发路径。

## What Changes

- **context-strategy**:**MODIFY**「请求前上下文策略钩子」—— `prepare` 签名加 `last_usage: Option<Usage>`(`Agent` 传上一轮 response 的 usage;首轮 `None`);**ADD**「Compacting 压缩策略」。
- **Compacting**:当 `last_usage.input_tokens > model_context_window × compact_trigger_ratio` 时,把 history 重写为 `System`(原 system prompt + 结构化 summary)+ 最近 `keep_recent_turns` 个**完整轮**原文;summary 由 provider 生成(结构化 prompt、`tools` 空)。未超阈值 = 等价 `Passthrough`。
- **agent-loop**:`Agent` 每轮记住 `response.usage`,下轮作 `last_usage` 传入 `prepare`。
- **config-layering**:**ADD** 压缩配置 `model_context_window`(`Option`,未配 = 压缩禁用)/ `compact_trigger_ratio`(默认 0.8)/ `keep_recent_turns`(默认 1)。
- **builtin-commands**:**ADD** `/compact` 手动触发(封装同一 `Compacting`,立即压一次)。
- **error**:`ContextError` → `AgentError::Context`(还掉 `add-context-strategy` 临时的 `→ProviderError::Transport` 映射)。

## Capabilities

### Modified Capabilities
- `context-strategy`: **MODIFY**「请求前上下文策略钩子」(`prepare` 加 `last_usage`)+ **ADD**「Compacting 压缩策略」。
- `config-layering`: **ADD**「上下文压缩配置」(window / ratio / keep_recent + 默认与校验)。
- `builtin-commands`: **ADD**「/compact 手动压缩」。
- `tui-shell`: **ADD**「会话 history 跨轮累积」(TUI 维护跨 prompt 共享 history,非每 prompt 重建)。

## Impact

- **code**:新增 `Compacting`(strategy)+ 结构化 summary 生成;`agent/mod.rs`(维护 `last_usage` + `prepare` 调用点 + `AgentError::Context`);`config`(新配置项 + 解析 / 默认 / 校验);`builtin-commands` / tui command(`/compact`);`Agent.provider` 共享给 strategy(`Box` → `Arc` 或注入独立句柄);**TUI** 以 `agent_history` 跨轮累积会话 history(偏离原「每 prompt 重建」)。
- **正确性红线**:① summary **MUST 入 `System`**(不引入额外 message),以保 `user`/`assistant` 交替(Anthropic 强制交替,连续同 role → `400`);② 保留窗口边界 **MUST 对齐完整轮**(从 `User` 处切),不切断 `assistant.tool_calls` ↔ `tool_result` 配对;③ `system` 永留。
- **降级**:summary 的 provider 调用失败 → **退回不压**(返回原 history,不致命);手动 `/compact` 失败可再手动一次;自动失败下轮再触发。
- **deps**:零新增。
