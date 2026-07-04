# auto-context-window-and-copy-hint

## Why

真机使用暴露两个小而高频的体验问题:

1. 压缩必须手配 `model_context_window` 才启用:未配时长会话默认无压缩直至撑爆 context,`/compact` 只回「压缩未启用」提示。对标产品(Claude Code / Codex CLI / aider / gemini-cli / opencode)全部是「内置模型元数据 + 配置覆盖出口」,无一要求用户手配。
2. 选区复制成功提示逐条追加 transcript Notice:高频复制刷屏(真机一屏 7 条「已复制 N 字」),且位置远离视线焦点(参照 Claude Code:输入框右上短暂提示)。

## What Changes

1. **压缩默认启用**:新增内置模型窗口表 `context_window_for(model)` 与解析链 `resolve_context_window`(显式配置 > 内置表匹配 > 保守默认 `65_536`);`Compacting` 触发判定改为按**当前 model** 判定时实时解析有效窗口(`/model`、`/models` 运行时切换自动跟随);装配层始终注入 `Compacting`,`AssembledAgent.compacting` 去 `Option`,「压缩未启用」提示删除。`model_context_window` 从必配项降级为覆盖出口(config 类型与 merge 行为不变)。
2. **复制提示挪位**:成功提示不再入 transcript,改为 activity line(输入框上方)右侧右对齐的短暂 hint(「已复制 N 字」,TTL 4s,靠既有 120ms tick 重绘过期,宽度不足时让位);复制失败仍走 transcript Notice(既有 spec 锁定,不动)。
3. **/compact 压缩进行态**(真机反馈修订):新 `Phase::Compacting`(activity line spinner「压缩上下文…」动画)与 `AgentEvent::CompactDone`(收场事件,计入排队推进闸门——扩为四事件);`/compact` 仅 Ready 且无排队可发起(否则 notice 拒绝);压缩期间提交按既有「消息排队」入可见队列、完成后自动推进;成功 notice 去消息数(「已压缩上下文」)。

## Impact

- Affected specs:
  - `config-layering`(MODIFIED 上下文压缩配置:未配 ≠ 禁用)
  - `context-strategy`(ADDED 上下文窗口解析;MODIFIED Compacting 压缩策略触发条件)
  - `builtin-commands`(MODIFIED /compact 手动压缩:去「未启用」分支)
  - `tui-shell`(ADDED 复制成功轻提示)
- Affected code: `src/provider/model_meta.rs`(新)、`src/agent/compacting.rs`、`src/app.rs`、`src/tui/{mod,clipboard,app,render}.rs`
- 内核部分(表 / 解析 / 触发 / 装配)强制 TDD 红绿;TUI 部分事后 TestBackend + insta 快照。
