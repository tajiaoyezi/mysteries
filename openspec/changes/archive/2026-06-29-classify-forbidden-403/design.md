## Context

OpenAI / Anthropic provider 均通过 `src/provider/transport.rs` 的 `classify()` 将 HTTP status 映射为 `ProviderError`。当前 `401 | 403` 合并为 `Auth`。

## Goals / Non-Goals

**Goals:**
- 403 与 401 语义分离:401 = 凭据无效;403 = forbidden(模型/配额/权限)。
- 最小 churn:不新增 `ProviderError` 变体,403 用既有 `Transport(String)` 携带人类可读文案。
- TUI 经 `AgentError` → `err.to_string()` 展示 transport 全文,不再显示「authentication failed」。

**Non-Goals:**
- 不改 429/5xx/重试策略。
- 不做 403 自动重试或换模型。

## Decisions

- **D1 403 → `ProviderError::Transport`**,message=`"{label} forbidden (403) — 模型无权限或配额,换模型或检查 key 权限"`。401 仍 `Auth`。
- **弃:新增 `Forbidden` 变体** — exhaustive match / TUI C7 分支 churn 大,本 change 不需要。
- **D2 测试放在 `openai.rs`**(已有 classify 测试套件);`transport.rs` 仅实现。

## Risks / Trade-offs

- 403 与 400/404 同为 `Transport`,靠 message 区分 — 可接受,与既有 4xx transport 一致。
