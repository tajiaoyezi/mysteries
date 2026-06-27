## ADDED Requirements

### Requirement: ProviderError 可恢复 / 致命错误分类

`ProviderError` SHALL 增补三个变体表达 §9 的「可恢复 vs 致命」语义,与既有 `Transport` / `Decode` 并列:`Auth`(鉴权失败,**致命**,不重试)、`RateLimited`(限流,**可重试**)、`Timeout`(超时,**可重试**)。新增变体 MUST 保持 `ProviderError` 既有的 `PartialEq` / `Eq` 派生(供测试断言);其语义为传输层重试策略提供判定依据 —— 致命变体终止、可重试变体进入退避重试。「何种具体条件 → 何变体」的映射属各传输实现(见 `openai-transport`),不在本抽象层固化。

> 背景:bootstrap design D9 预告了 `Auth` / `RateLimited` / `Timeout` 「要在有真实调用时才有构造点」;`add-openai-live-transport` 即其构造点。既有 `Transport`(§9)/ `Decode`(归一化解析失败)语义不变。

#### Scenario: 致命变体终止、可重试变体退避重试

- **WHEN** 传输层产出 `ProviderError::Auth`
- **THEN** 重试策略将其判为致命、不重试,直接上抛

#### Scenario: 可重试变体进入重试

- **WHEN** 传输层产出 `ProviderError::RateLimited` 或 `ProviderError::Timeout`
- **THEN** 重试策略将其判为可重试,触发指数退避重试(至上限后方上抛)
