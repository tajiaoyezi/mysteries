# 2026-07-12 · 61 · archive modernize-ratatui-and-deny-unsound

## 决策
- TUI 依赖迁移采用 `ratatui 0.30.2` 最小 feature 与单一 `crossterm 0.29.0` | 选:`ratatui(default-features=false, features=[crossterm_0_29, layout-cache])`，直接 `crossterm` 保留 defaults + `event-stream` | 弃:启用 Ratatui 全部 defaults(扩张 widgets/macros 能力面)、保留直接 crossterm 0.28(形成双版本且 backend 仍选 0.29)、拆分直依赖到 Ratatui 子 crate(全仓 import 重写) | 主导:讨论收敛 | 依据:Cargo feature graph / dependency tree / compiler
- soundness 债务通过上游兼容迁移消除，不在仓库内 patch 或关闭 layout cache | 选:移除 `paste 1.0.15` 与 `lru 0.12.5`，接受 Ratatui layout cache 的安全 `lru 0.18.1` 及 crossterm defaults 新增的 `derive_more` 传递路径 | 弃:`[patch]` 替换单个依赖(承担上游兼容责任)、关闭 layout cache(无基准支持的性能退化)、全量 `cargo update`(无关 churn) | 主导:依赖审查收敛 | 依据:lockfile diff / cargo tree / RustSec
- 视觉迁移按“主体纯 port + 一份受控 adapt”验收 | 选:只接受 `command_completion_snapshot` 中相邻同 style run 合并与 `/models` 完整描述，并由用户逐项批准 | 弃:为复刻 0.29 缺字/留白编写兼容 hack、批量 approve 所有上游快照差异、借依赖升级重设计 C1–C11 | 主导:用户批准 | 依据:设计规范 / `insta` diff / Windows Terminal 真机
- RustSec 策略只新增 `--deny unsound`，vulnerability 与 unsound hard-fail，unmaintained 继续可见但首版不阻断 | 选:固定 `cargo-audit 0.22.2` 的绝对 binary 扫描根 lockfile，并以命令策略 + exit 0 证明 0 vulnerability / 0 unsound | 弃:`--deny warnings` / `--deny unmaintained`(把 `bincode` 治理混入本 change)、advisory ignore / 输出过滤(制造假绿)、项目 Cargo alias dispatch(可被 shadow) | 主导:安全审查收敛 | 依据:新旧 lockfile 正反向审计 / PR 与 master workflow logs
- API 适配严格由编译器和回归测试暴露，最终不为凑改动触碰 `src/tui/` 或 `src/cli.rs` | 选:保留现有 Press-only、ConPTY paste、滚动/选择、权限、auth selector 与 terminal restore 行为，仅更新依赖、门禁、文档和经批准快照 | 弃:预先重构状态机、主动采用 `Event::Paste`、改变 CLI flags / session wire / 权限语义 | 主导:spec 约束 | 依据:code diff / 全量 tests / 真机验收
- 远端证据按“最新 PR head → 实现 merge SHA → 最终证据 merge SHA”逐层确认，不用旧 SHA green 代替 | 选:PR #4 合入实现、PR #5 合入 post-merge 证据，并确认最终 `master` CI 与 Security audit | 弃:在同一 PR 内反复勾选“本 PR checks 已绿”造成 head/证据循环、用本地成功替代远端双平台结果 | 主导:讨论收敛 | 依据:PR checks / Security audit run / tasks 32/32

## 变更
- `Cargo.toml` / `Cargo.lock` 迁移到 `ratatui 0.30.2`、单一 `crossterm 0.29.0` 与 `lru 0.18.1`，移除 `paste 1.0.15` 和受 `RUSTSEC-2026-0002` 影响的 `lru 0.12.5` 路径。
- `security-audit` workflow 的绝对 binary 调用增加 `--deny unsound`；当前根 lockfile 审计为 0 vulnerability / 0 unsound，同时保留 `syntect -> bincode 1.3.3` 的 `RUSTSEC-2025-0141` unmaintained warning。
- 保持 Agent Loop、Provider、Tool、Permission、Session、Config 与 CLI 公共行为不变；仅更新经用户批准的 `command_completion_snapshot`，无其他快照 churn、无 `.snap.new`。
- 更新 CONTRIBUTING、README、CHANGELOG 与 `dependency-security` / `tui-shell` / `cli-runtime` delta specs；fmt、clippy、全量 test、release build、strict OpenSpec、Windows Terminal 真机、PR checks 与最终 master audit 全部通过。

## 待决
- `actions/checkout` / `actions/cache` 的 Node runtime、不可变 SHA、普通 CI 权限与 cache 行为由紧随其后的 `modernize-github-actions-runtime` 独立 change 处理。
- `syntect -> bincode 1.3.3` 的 `RUSTSEC-2025-0141` unmaintained warning 继续可见，需独立治理或明确 owner、复查条件与移除路径；在此之前不启用 `--deny unmaintained`。
- 通用 Agent cancellation、ExecutionScope、subagent 与 MCP 继续保持独立路线，不由本次依赖迁移扩张。

## 引用
- OpenSpec change:`modernize-ratatui-and-deny-unsound`;spec:`dependency-security`、`tui-shell`、`cli-runtime`。
- GitHub:PR #4(实现)、PR #5(合入后证据);最终 `master` CI 与 Security audit。
- 关联决策:`2026-07-11-60-archive-harden-dependency-security.md`、`2026-07-11-59-archive-parallelize-safe-tool-calls.md`。
