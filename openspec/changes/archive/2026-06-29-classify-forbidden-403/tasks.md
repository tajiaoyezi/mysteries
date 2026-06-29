## 1. classify 纯函数(TDD)

- [x] 1.1 【红】改 `classify_auth_statuses_as_fatal_auth`:401→Auth 保留;403 断言改为 fatal `Transport` 且 message 含 `forbidden (403)`;运行确认失败
- [x] 1.2 【绿】`transport.rs`:401→Auth;403→`Transport("{label} forbidden (403) — …")`
- [x] 1.3 【重构】清理

## 2. spec + 校验

- [x] 2.1 openai-transport delta spec(401/403 分场景)
- [x] 2.2 `cargo test` + `cargo clippy --all-targets -D warnings`
- [x] 2.3 `openspec validate classify-forbidden-403 --strict`
