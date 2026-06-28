## Context

- 地基(已 archive):`ModelResponse.usage: Option<Usage>`(真实 token 用量);`ContextStrategy { async fn prepare(&self, history: &[Message]) -> Result<Vec<Message>, ContextError> }` + 默认 `Passthrough`;`Agent` 持 `Box<dyn ContextStrategy>` + `set_strategy`。
- `add-context-strategy` 决策记录(18)已预告:压缩接入真实用量时**扩 `prepare` 签名加 `last_usage`**;并把临时 `From<ContextError> → ProviderError::Transport` 细化为 `AgentError::Context`。本 change 兑现两者。
- 目标:贴近 claude code 的 `/compact`——重度结构化 summary + 自动/手动双触发。

权威次序:code / 测试 > spec。

## Goals / Non-Goals

**Goals:** 真实(调 provider)上下文压缩;配置驱动阈值;自动 + 手动 `/compact`;失败降级不致命。

**Non-Goals:** 不引 tokenizer(阈值用 provider 真实 `usage`);不做 token 级精确预算保留(用「完整轮」近似);不做持久化 / 多 provider 压缩(1.2+)。

## Decisions

### ① usage 流入:扩 prepare 签名加 last_usage

- `prepare(&self, history: &[Message], last_usage: Option<&Usage>) -> Result<Vec<Message>, ContextError>`。`Agent` loop 每轮请求后记 `response.usage`,**下一轮**请求前作 `last_usage` 传入(首轮 `None`)。`Passthrough` 忽略它(签名变、body 不变,零回归)。
- 为何用 `last_usage.input_tokens`:它 = 上一轮请求 prompt 编码后的真实 token 数 ≈ 当前 history 的 token(差一轮),是无 tokenizer 下最准的「当前上下文大小」信号。

### ② 触发阈值:配置 window × ratio

- 配置 `model_context_window: Option<u32>`(tokens)、`compact_trigger_ratio: f32`(默认 0.8)。触发条件:`last_usage.input_tokens > window × ratio`。
- `model_context_window` **未配 → 压缩禁用**(装配 `Passthrough`,安全默认 = 现状不变);配了才装 `Compacting`。装配在 cli-runtime / app 层据 config 选 strategy。
- `ratio` 留 buffer(默认 0.8 = 触发后还有 ~20% 空间容纳 summary 请求本身)。

### ③ 压缩形态:claude code 式 —— System(原 + summary) + 最近 keep 轮

- 重写为:`[ System(原 system prompt + "\n\n# 此前对话摘要\n" + summary), <最近 keep_recent_turns 个完整轮原文> ]`。
- `keep_recent_turns` 默认 **1**(贴 claude code 的重度压缩,只留最近一轮保当前任务连续;可配 0 = 全压)。
- **summary 注入 `System` 而非独立 message** —— 关键正确性:Anthropic 强制 `user`/`assistant` 交替,若 summary 作独立 `User` 又紧跟最近轮的 `User` 会连续同 role → `400`。拼进 `System` 既不破坏交替,又让 summary 成为高优先级 context。

### ④ summary 生成:Compacting 调 provider

- `Compacting` 持 provider 句柄 + model(见 ⑧),压缩时以**结构化 prompt** 调 `provider.complete`(`tools` 空、`max_tokens` 限制):指示模型把被压区间总结为分节摘要——**已完成工作 / 当前文件与代码状态 / 关键决策 / 下一步待办**(仿 claude code,信息保真)。被压区间 = `system` 之后到「最近 keep 轮」之前的全部消息。

### ⑤ 正确性红线(约束)

- **保留窗口边界对齐完整轮**:从末尾向前数 `keep_recent_turns` 个 `User` 消息为界(`User` 标志一轮起点),界**前**全部压、界**后**(含该 `User`)原样留 —— 边界落在 `User` 前,天然不切断 `assistant.tool_calls` ↔ `tool_result`。
- **summary 入 `System`**(见 ③),保 user/assistant 交替。
- `system` 永留;summary 可被下次压缩再压(幂等:下次把 "上次 summary 所在 system + 新累积" 重新压)。

### ⑥ 降级:失败不致命

- summary 的 `provider.complete` 失败 → `Compacting::prepare` **退回不压**(返回原 history 克隆,`Ok`),**不**让一轮对话挂掉。自动路径:本轮不压、下轮 `last_usage` 仍超阈值会**再触发**。
- 手动 `/compact` 路径:失败回一条 notice(可再手动一次)。
- `ContextError` → `AgentError::Context`(还掉临时 `→Transport`);但因 Compacting 内部降级,正常流程不上抛 —— `Context` 变体留作接口完备(如配置非法等不可降级情形)。

### ⑦ 手动 /compact(builtin-commands)

- `/compact` 命令:立即对当前 history 跑一次 `Compacting`(无视阈值,`last_usage` 不参与判定、直接压),封装**同一**压缩逻辑(压缩区间 / summary / 入 System 一致)。结果替换会话 history,回一条 notice(压缩前后消息数 / 是否降级)。
- 与 `/model` 等既有命令同构,走 builtin-commands 既有解析 / 执行语义。

### ⑧ provider 共享给 strategy

- `Compacting` 需 provider 生成 summary。现 `Agent.provider: Box<dyn Provider>`。方案:改为 `Arc<dyn Provider>`,`Agent` 与 `Compacting` 共享同一句柄(同一 model / 凭据)。装配时(window 已配)构造 `Compacting::new(provider.clone(), model, cfg)` 注入 `Agent::set_strategy`。
- 备选(弃):给 `Compacting` 独立构造 provider —— 重复装配凭据 / base_url,易漂移。共享 `Arc` 最省。

## Risks / Trade-offs

- **summary 调用花 token / 钱 / 延迟**:触发时多一次 provider 往返;`ratio` 留 buffer 确保该请求不再撞上限。可接受(压缩本质如此)。
- **keep=0 全压时丢最近精确上下文**:默认 keep=1 缓解;靠结构化 summary 保真。贴 claude code 的取舍。
- **`last_usage` 滞后一轮**:阈值判定差一轮,极端单轮暴涨可能略晚触发;`ratio` buffer 吸收。无 tokenizer 下可接受。
- **summary 质量依赖模型**:prompt 结构化 + 分节降低漂移;失败有降级兜底。
- **provider `Box`→`Arc` 改动**:触及 `Agent` 构造与 cli-runtime 装配;以零回归测试锁定既有行为不变。
