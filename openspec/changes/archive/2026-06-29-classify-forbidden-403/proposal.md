## Why

codeplan / OpenAI 兼容端点返回 HTTP 403(模型无权限、配额或 scope 不足)时,共享 `classify()` 将其与 401 一并映射为 `ProviderError::Auth`,TUI 显示「鉴权失败 / provider authentication failed」,误导用户去换 key,而非换模型或检查权限。

## What Changes

- `transport::classify`:401 仍 → `Auth`;403 → 带明确文案的 fatal `Transport`(非 Auth)。
- Anthropic 复用同一 `classify`,行为同步。
- 更新 openai-transport spec 中 401/403 分类 requirement;补 classify 单测。

## Impact

- Affected specs: `openai-transport`(MODIFIED)
- Affected code: `src/provider/transport.rs`, `src/provider/openai.rs` 测试
