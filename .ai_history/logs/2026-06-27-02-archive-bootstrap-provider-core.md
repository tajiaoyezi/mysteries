# 2026-06-27 · 02 · archive bootstrap-provider-core

## 决策

- **第一个 change 取内核地基**(Provider 抽象 + 协议归一化 + Mock + 单轮 stdout),非 TUI | 选:§12 第 1 步内核优先 | 弃:先做 TUI 骨架(违背 §12「先内核后外壳」)| 主导:主 agent 把关(纠正会话中随口的 TUI 建议)| 依据:技术方案 §12
- **范围取 Option A:只做 OpenAI 归一化,不接 live HTTP / 凭据** | 弃:Option B(本 change 即接 reqwest + env key;传输与凭据天然耦合,放一起更干净)| 主导:实现子 agent propose 时与用户确认 | 依据:change design.md D3
- **Provider / DeltaSink 契约采 §5.1 的 `&self` + `Send+Sync` + interior mutability** | 弃:`&mut self` / `&mut dyn`(把独占借用泄漏进核心契约,堵死后续共享 / 并发)| 主导:主 agent 审查发现实现期静默偏离 → 出修复 prompt → 子 agent 修正 | 依据:change design.md D13、§5.1
- 其余定案(省略 `tools` 字段、`Decode` 增补、`FinishReason` 兜底等)见 change design.md D1–D13,不复述

## 变更

- 新增 Rust crate;`provider` / `agent::message` / `error` 模块;OpenAI wire 归一化 + `MockProvider` + `run_single_turn` + `main` stdout demo(commit `5cbf216`)
- archive:`changes/bootstrap-provider-core` → `changes/archive/2026-06-27-bootstrap-provider-core`;`specs/` 落地 `provider-abstraction`(5 requirement)+ `conversation`(1 requirement)
- 验证:`cargo test` 10 passed;`cargo build` 通过(dead_code warning 属契约领先消费者,接受,transport change 自然清零)

## 待决

- transport change:reqwest + SSE 累积(§5.2)、超时 / 重试、凭据链(§5.6,是否引 `secrecy`)、live endpoint smoke
- 归一化目前仅以 JSON fixture 验证,无真实 endpoint 校验

## 引用

- change:`bootstrap-provider-core`(rationale / rejected alternatives 全量见其 design.md D1–D13;archive 路径 `changes/archive/2026-06-27-bootstrap-provider-core`)
- 技术方案 §5.1 / §5.5 / §9 / §10 / §12
- session log:无专属 checkpoint —— 本 change 由子 agent propose / implement,主 agent 负责 review 与契约修正
