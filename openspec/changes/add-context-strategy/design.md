## Context

现状(已对代码核实):

- `src/agent/mod.rs` 的 `run_observed` 每轮直接 `messages: history.clone()` 构造 `ModelRequest`(强制收尾那次亦然);`run_single_turn` 同样手工构造 messages。
- agent 目录仅 `mod.rs` + `message.rs`,**无** `context.rs` / `session.rs`。
- 技术方案 §13 承诺的 `ContextStrategy` 缝实际缺失。

权威次序:code > spec / 计划。本 change 据实补建该缝。

## Goals / Non-Goals

**Goals:**
- 建立请求前可替换的 `ContextStrategy` 钩子。
- `Passthrough` 行为等价现状(零回归)。
- 为 `add-token-compaction` 预留注入点。

**Non-Goals(本 change 明确不做):**
- 不实现任何压缩 / 截断 / summary(留 `add-token-compaction`)。
- 不引入 token 计量(那是 `expose-token-usage`;本 change **不依赖**它,trait 签名不含 `usage`,见 ①)。
- 不碰持久化 `Session`(1.2)。

## Decisions

### ① trait 形状:async + `history → Vec<Message>`,不带 usage 参数

- `#[async_trait] pub trait ContextStrategy: Send + Sync { async fn prepare(&self, history: &[Message]) -> Result<Vec<Message>, ContextError>; }`
- **async**:为压缩实现预留——`Compacting` 要 `await` provider 生成 summary。现在就 async 化,避免 `add-token-compaction` 阶段把整条调用链改 async(侵入式重构)。`Passthrough` 的 async 体无 `await`,可接受(平凡实现,沿用既有 `#[async_trait]` 模式)。
- **不带 usage 参数**:`Passthrough` 不需要 usage;为其加入参违反 YAGNI 且本 change 无法测。`add-token-compaction` 接入真实用量时,**预计扩展** trait 签名(加 `last_usage` 或改 stateful strategy)——这是「实现演进时的签名微调」,属**已知边界**:本 change 坐实的是「请求前过一遍」的**结构缝**,用量如何喂入留待压缩 change 定。如此 trait 不引用 `expose-token-usage` 的 `Usage`,二者**真并行**。
- **`Result` + `ContextError`**:为压缩失败的降级通道预留;`Passthrough` 恒 `Ok`。`ContextError` 本 change 仅最小定义。

### ② 为何不 MODIFY agent-loop「多轮编排循环」

- 接线后每轮以 `strategy.prepare(&history).await?` 的结果请求;`Passthrough` 下其结果 == `history.clone()`,与既有「以完整 history 请求」**逐字节等价**。故 agent-loop 既有 requirement 与其 scenario 仍真,**不 MODIFY**;以「零回归」(既有 agent-loop 全测保持绿)锁定等价性。
- 新增能力(可替换钩子)归入**新 capability** `context-strategy`,边界清晰,且利于与 `expose-token-usage` 并行——不碰 agent-loop spec 文件。

### ③ 默认装配与注入入口

- `Agent` 默认 `strategy = Box::new(Passthrough)`;不改任何现有构造 `Agent` 的调用点行为(默认即 Passthrough)。
- 加**最小**注入入口(`Agent::set_strategy(&mut self, Box<dyn ContextStrategy>)` 或构造参数),仅供 `add-token-compaction` 注入 `Compacting`;不暴露超出压缩接入所需的配置面。

### ④ Passthrough 测试不碰 ModelResponse(并行最小交叠)

- `Passthrough` 核心行为 = `prepare(history) == history`,用**纯 `Vec<Message>`** 单元测试,不经 loop、不构造 `ModelResponse`。
- Loop 接线的零回归由**既有** agent-loop 测试覆盖(它们已在 master;本 change 不新增 `ModelResponse` 字面量)。以此把与 `expose-token-usage` 在 `agent/mod.rs` 的交叠压到最小(对方改测试模块的 `ModelResponse` 构造,本 change 改 loop 主体 + 末尾新增纯 Message 测试,两不相邻)。

## Risks / Trade-offs

- **async trait 平凡实现(Passthrough 无 await)**:可接受;clippy 若警告,以既有 `#[async_trait]` 模式消解。
- **trait 签名未含 usage,`add-token-compaction` 可能扩签名**:已在 ① 记录为已知边界。本 change 只承诺「结构缝就位」,不承诺「签名终态冻结」——比现在臆测 usage 入参形状更稳(YAGNI)。这意味着 §13「1.1 纯换实现」对 trait 签名而言仍会有一次微调,如实记录。
- **与 `expose-token-usage` 的 agent/mod.rs 交叠**:已由「Passthrough 不构造 ModelResponse」(④)单向压制;merge 由主 agent 收口。
