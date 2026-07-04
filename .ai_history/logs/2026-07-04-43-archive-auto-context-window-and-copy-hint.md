# 2026-07-04 · 43 · archive-auto-context-window-and-copy-hint

## 决策

- 压缩默认启用,窗口解析链 = 显式配置 > 内置表 > 保守默认 65_536 | 选:手写小表(16 条)+ 长 pattern 子串 / 短 pattern(o 系)边界匹配 | 弃:启动探测 `/v1/models` 的 `context_length`(仅 OpenRouter/vLLM/Ollama 类网关有,官方 OpenAI/Anthropic 无,加启动网络依赖)、打包 models.dev / litellm 大目录(外部数据源依赖) | 主导:用户提出「不想手配」并要求对比同类产品,方案对比(Claude Code / Codex CLI / aider / gemini-cli / opencode 全为「内置元数据 + 覆盖出口」)后拍板 | 依据:code/tests
- 保守默认取小不取大:估小 → 压缩偏早(多一次 summary,无害);估大 → 压缩缺席直至 provider 超限报错 | 主导:讨论收敛
- 有效窗口在**判定时**按当前 model 解析(非构造时固定),`/model`、`/models` 切换自动跟随 | 依据:`Compacting` 既有 `set_model` 接线,零新机制
- 复制成功提示挪 activity line 右侧短暂 hint(TTL 4s,靠既有 120ms tick 无条件重绘过期,不加定时器;宽度不足整体让位) | 弃:成功也留 transcript(高频刷屏,真机一屏 7 条「已复制」) | 主导:用户(参照 Claude Code 输入框右上形态);失败仍留 transcript Notice(spec 已锁、异常留痕)
- /compact 进行态(真机反馈修订):`Phase::Compacting` + `AgentEvent::CompactDone`,推进闸门三事件扩四(TurnComplete/Interrupted/Error/CompactDone),发起门控 Ready 且无排队 | 弃:收场复用 `TurnComplete`(按"轮"清 iteration/turn token、bump 计数,语义不属压缩)、运行中 /compact 静默排 channel 延迟执行(原行为,不可见易惊讶,改门控拒绝 + notice) | 主导:用户真机反馈(无动画 / 计数多余 / 期间提交要排队)
- 成功 notice 去消息数:「已压缩上下文」 | 主导:用户

## 变更

- 新增 `src/provider/model_meta.rs`:`WINDOW_TABLE`(顺序敏感,特定在前)+ `context_window_for` + `resolve_context_window`;`CompactionSettings.model_context_window` 转 `Option<u32>`(= 显式覆盖);`AssembledAgent.compacting` 去 `Option`,`run_compact_command(&Compacting, ..)`,「压缩未启用」提示与分支删除
- `clipboard.rs` 成功路径 → `AppState.copy_hint`(`active_copy_hint(now)` 纯函数按 TTL 过滤);`render_activity` 右对齐 hint
- `Phase::Compacting`(activity line spinner「压缩上下文…」,accent、无 esc 提示)、`CompactDone` 收场(成功/失败均发)与排队推进;`/compact` 发起门控
- spec:config-layering(未配 ≠ 禁用)、context-strategy(ADDED 窗口解析 + Compacting 触发条件改写)、builtin-commands(/compact 重写:门控/动画/收场/无计数)、tui-shell(ADDED 复制轻提示;运行中可中断 + 消息排队闸门措辞扩四事件)
- 测试 487 → 503;红绿证据:model_meta 6 测 `not yet implemented` 红、切 model 跟随断言级红(适配层未接表)、notice 去计数断言红(左值含 `8 → 3 条消息`);快照新增 2(copy_hint / compacting),既有零漂移

## 待决

- 窗口表会随新模型过时:接受为全行业通行代价,`model_context_window` 覆盖兜底;新模型出现时加行即可
- 表值为保守近似(如 deepseek 65_536、legacy gpt-4 8_192);网关侧实际窗口不同时用配置覆盖
- 压缩不可中断(v1 Non-Goal):Compacting 期间 Esc 无效果(`Interrupt` 于下轮 `Prompt` 前 drain,快速连按仍可清排队);真机若嫌压缩耗时再议可中断
- 自动压缩(轮中 `prepare` 触发)无独立动画:发生于 CallingModel 态内,被既有 spinner 覆盖,暂不单独提示

## 引用

- OpenSpec change:`auto-context-window-and-copy-hint` → archive/2026-07-04-auto-context-window-and-copy-hint(propose bc46126)
- 相关 log:[[2026-07-03-39-archive-add-message-queue]](推进闸门三事件源头,本次扩为四)、[[2026-07-01-35-archive-fullscreen-mouse-select-copy]](复制成功轻提示最初记为待决处)
- 跨越 session:本会话(1.0 收口后首个 change;两项小优化 + 一轮真机反馈修订,主 agent 直接实施)
