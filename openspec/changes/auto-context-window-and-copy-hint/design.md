# Design — auto-context-window-and-copy-hint

## D1 窗口解析链:显式配置 > 内置表 > 保守默认

`resolve_context_window(explicit: Option<u32>, model: &str) -> u32`,默认 `DEFAULT_CONTEXT_WINDOW = 65_536`。

- 保守默认**取小不取大**:窗口估小 → 压缩偏早(多一次 summary 调用,无害);估大 → 压缩缺席直至 provider 报 context 超限(坏)。方向上宁可错小。
- **被否**:启动时从 `/v1/models` 探测 `context_length` —— 仅 OpenRouter / vLLM / Ollama 类网关提供,官方 OpenAI / Anthropic 均无,不可靠且加启动网络依赖;后续可作增强层,v1 不做。
- **被否**:打包 models.dev / litellm 大目录 —— 引入外部数据源依赖;离线红线内手写小表覆盖主流已足够,配置覆盖兜底长尾。

## D2 内置表与匹配规则

表为 `&[(&str, u32)]` 常量,**顺序敏感**(更特定条目在前:`gpt-4.1` / `gpt-4o` / `gpt-4-turbo` 先于 `gpt-4`);输入 lowercase 后匹配,首个命中生效。

- pattern 长度 > 2:`contains`(容忍网关前缀名,如 `openai/gpt-4o`、`wps-gpt-4o`);
- pattern 长度 ≤ 2(`o1` / `o3` / `o4`):边界匹配(全等 / `{p}-` 起头 / 含 `/{p}`),防 `yi-o1` 类子串误伤。
- 取舍:表会随新模型过时 —— 接受(全行业通行代价),`model_context_window` 覆盖为兜底出口。

## D3 Compacting 判定时解析(非构造时固定)

`CompactionSettings.model_context_window: u32 → Option<u32>`(语义 = 显式覆盖);`exceeds_threshold` 内以 `resolve_context_window(explicit, &self.model)` 求有效窗口。`Compacting` 已有 `set_model` 且 `/models` 切换路径已接线(tui/mod.rs `apply_set_provider`),窗口自动跟随,零新接线。

- 双实例现状不动(Agent 内 strategy 实例 + task 侧 `/compact` 句柄实例,二者经同一切换路径同步),本 change 仅去 `Option`。
- `run_compact_command(Option<&Compacting>, ..) → (&Compacting, ..)`,「压缩未启用」分支删除(压缩句柄随 agent provider 始终存在,「无 provider」情形不可达)。

## D4 复制 hint 放 activity line 右侧

`AppState.copy_hint: Option<CopyHint { text, set_at }>` + `active_copy_hint(now)`(TTL 过滤,纯函数)+ `COPY_HINT_TTL = 4s`。`render_activity` 在左侧活动 spans 之外右对齐渲染 hint;`左宽 + 1 + hint 宽 > 行宽` 时 hint 让位跳过。

- 过期靠既有 120ms spinner tick 的**无条件重绘**(主循环每次 select 后必 draw),不加新定时器、不在 tick 里清状态(render 侧按 TTL 过滤即可,残留 `Some` 无害)。
- 失败仍 transcript Notice:spec 已锁(「复制失败静默降级为 Notice」),且异常应留痕;刷屏来源是高频**成功**提示。
- **被否**:成功也留 transcript(现状,刷屏);hint 放状态栏(底部状态栏语义为会话元信息,且宽度更挤)。

## D5 /compact 压缩进行态(真机反馈修订)

真机反馈三点:压缩期间无任何反馈动画;完成 notice 带消息数(不要);压缩期间提交应进可见队列。修订:

- 新 `Phase::Compacting` + 新 `AgentEvent::CompactDone`:`/compact` 发起(仅 Ready 且无排队,否则 notice 拒绝)置 `Compacting`;收场(成功 / 失败都发)置回 `Ready` 并计入排队推进闸门(扩为四事件:TurnComplete / Interrupted / Error / CompactDone)。
- 动画复用既有 spinner 体系(「⠸ 压缩上下文…」,accent 样式、无 esc 提示);排队复用「消息排队」的 phase 分流(`Compacting.is_running() == true`),零新机制。
- 成功 notice 去计数:「已压缩上下文」。
- **被否**:收场复用 `TurnComplete`——它按"轮"清 `iteration` / turn token、bump 新消息计数,语义不属压缩;单独事件干净且 spec 可锁。
- **被否**:运行中 `/compact` 静默排到 channel 轮后执行(现状)——延迟执行不可见、易惊讶,改为门控拒绝 + notice。
- **Non-Goal(v1)**:压缩不可中断(期间 `Interrupt` 无效果,于下一轮 `Prompt` 前被 drain;快速连按仍可清排队)。

## 测试断点

- model_meta:解析优先级 / 大小写不敏感 / 特定条目遮蔽(`gpt-4.1` vs `gpt-4`)/ o 系边界匹配 / 未知模型默认 65_536,纯单测。
- compacting:`Some` 覆盖生效、`None` 走表、`set_model` 后同一 `last_usage` 翻转触发、`run_compact_command` 非 Option(删「未启用」测试)。
- 装配:未配 window 也注入 `Compacting`(改写既有 Passthrough 断言测试)。
- TUI:clipboard 两个既有测试改断 hint + transcript 不增;TTL 存续 / 过期 / 覆盖;快照:activity line 带 hint(新增)、既有快照零漂移。
