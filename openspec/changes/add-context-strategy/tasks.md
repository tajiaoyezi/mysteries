# Tasks — add-context-strategy

> TDD:agent 内核纯逻辑,**强制红-绿**。
> 🔴 **红灯停点①**:`ContextStrategy` trait + `Passthrough` 首次成型(新 trait)——测试写完、贴出**运行时**失败输出后**停下等确认**,再写绿。
> 红灯构造为「运行时红」:先加 trait + `Passthrough` 最小桩(`prepare` 返回**空** `Vec` 桩)使其编译,测试断言 `prepare(history) == history` → 运行时失败(空 vs 非空)。
> 接线(任务 2)为**重构**(行为不变),不走红-绿,以既有 agent-loop 测试**保持绿**验证零回归。

## 1. trait + Passthrough(context-strategy,强制 TDD)

- [x] 1.1 【红】先只写测(纯 `Vec<Message>`,不构造 `ModelResponse`):以多条 `Message`(System / User / Assistant / ToolResult)构造 history,断言 `Passthrough::prepare(&history).await` 返回的 `Vec<Message>` 与 history **逐条相等**(顺序与内容一致)。运行确认失败(桩返回空 vec → 运行时红)。
- [x] 1.2 🔴 **红灯停点①**:贴出 1.1 测试代码 + 失败输出,**停下等确认**(新 trait 首次成型)。
- [x] 1.3 【绿】最小实现:`src/agent/context.rs` —— `#[async_trait] trait ContextStrategy: Send + Sync { async fn prepare(&self, history: &[Message]) -> Result<Vec<Message>, ContextError>; }`;`Passthrough`(克隆返回 history);`ContextError` 最小定义。`agent/mod.rs` 加 `mod context;` 并 re-export。

## 2. Agent 接线(重构,零回归)

- [x] 2.1 `Agent` 加 `strategy: Box<dyn ContextStrategy>` 字段,默认 `Box::new(Passthrough)`;现有构造 `Agent` 的路径默认即 Passthrough。
- [x] 2.2 `run_observed`(及强制收尾那次、`run` 委托链)每轮请求前改为 `let msgs = self.strategy.prepare(&history).await?;`,以 `msgs` 构造 `ModelRequest`。`ContextError` 经 `?` 上抛(映射到既有 `AgentError`;Passthrough 不触发)。
- [x] 2.3 加最小注入入口 `Agent::set_strategy`(供 `add-token-compaction`);本 change 不写非 Passthrough 实现。
- [x] 2.4 零回归:既有 agent-loop 全部测试(自然终止 / 多轮编排 / 强制收尾 / observer / set_model)**保持绿**,证明接线行为等价。

## 3. 收尾验证

- [x] 3.1 `cargo build` 通过;`cargo test` 全绿(新红-绿 + 零回归)。
- [x] 3.2 `openspec validate add-context-strategy --strict` 通过。
- [x] 3.3 `cargo clippy --all-targets -- -D warnings` 零警告;`cargo fmt --check` 净。
