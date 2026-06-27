# 2026-06-27 · 03 · archive add-agent-loop-core

## 决策

- **第二个 change 取 §12 第 2 步「Loop 核心」半**(2-split:Loop + 抽象先,7 实体工具留 change B)| 选:Loop 核心先行 | 弃:Loop + 实体工具一把梭(过大)| 主导:主 agent 出 prompt 时给拆分选项 → 子 agent propose 采纳 | 依据:change design.md DA9
- **权限门与 §3 oneshot / UI 解耦,做成可注入 `PermissionDecider` seam** | 弃:本 change 即接 §3 oneshot(TUI 耦合,过早)| 主导:主 agent prompt 指定 | 依据:DA4
- **补回 change 1 D5 省略的 `ModelRequest.tools` + OpenAI tools 序列化** | 主导:范围内自然项 | 依据:DA5
- **契约延续 change 1 的 `&self` + `Send+Sync`;新增 `impl Provider for Arc<T>` 支持共享 provider** | 主导:子 agent 实现,主 agent 审查时确认并要求补记 DA11 | 依据:DA11
- 其余(`Message` 加 `Clone`、`AgentError`、`Vec` registry 等)见 change design.md DA1–DA11,不复述

## 变更

- 新增 `agent::Agent` + `run`(多轮编排 / 终止条件 / `max_iterations` / 6 类事件入 history)、`tool` 系统抽象、`permission` 门;`ModelRequest.tools` + wire tools 序列化;`AgentError`;**零新依赖**(commit 见 feat)
- 验证:`cargo test` 22 passed;dead_code warning 10→22(`main` 仍单轮,本 change Non-Goal,接 Loop 后清零,接受)
- archive:`changes/add-agent-loop-core` → `changes/archive/2026-06-27-add-agent-loop-core`;`specs/` 新增 `agent-loop` / `tool-system` / `permission-gate`,`provider-abstraction` ADDED(tools 下发)

## 待决

- change B:7 实体工具(read / list / glob / grep / write / edit / shell)+ tempdir 测试 + 输出截断;`main` 改接 Loop + stdin y/n decider
- 审查 ② 项:`ToolRegistry` 用 `Vec` 允许重名工具(`schemas` 会把重名都下发给模型)—— change B 注册实体工具时定 dedup / 换 `HashMap`
- `run` 是否最终也返回完整 history(design OQ)

## 引用

- change:`add-agent-loop-core`(rationale / rejected alternatives 全量见 design.md DA1–DA11;archive 路径 `changes/archive/2026-06-27-add-agent-loop-core`)
- 技术方案 §5.3 / §5.4 / §5.5 / §6 / §9 / §10 / §12
- 前置 change:`bootstrap-provider-core`(决策记录 2026-06-27-02)
- session log:无专属 checkpoint —— 子 agent propose / implement,主 agent 负责 review 与出修复 / 拆分 prompt
