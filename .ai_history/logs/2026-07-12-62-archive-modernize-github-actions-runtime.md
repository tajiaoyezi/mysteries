# 2026-07-12 · 62 · archive modernize-github-actions-runtime

## 决策
- GitHub Actions runtime 升级至受支持版本并使用不可变 SHA | 选:`actions/checkout@v7.0.0` 与 `actions/cache@v6.1.0` 的固定 SHA，统一使用 Node.js 24 runtime | 弃:继续使用旧版 Action(runtime 已过时)、使用 floating tag(无法保证供应链可复现) | 主导:讨论收敛 | 依据:官方 release / `action.yml` / CI 验证
- workflow 权限采用最小权限 | 选:`permissions: contents: read` 与 `persist-credentials: false` | 弃:沿用默认权限(边界不明确)、授予额外写权限(当前任务不需要) | 主导:讨论收敛 | 依据:workflow 实际行为 / Security workflow
- 使用显式 revision marker 证明被测试 revision | 选:输出唯一 `TESTED_REVISION`，并核对 PR synthetic merge、implementation merge、evidence merge 与 review-fix merge | 弃:仅使用 REST API `run.head_sha` 推断 merge SHA(语义不足)、让 evidence 自证自身提交(形成循环证明) | 主导:讨论收敛 | 依据:GitHub Actions job log / Git object parent 关系
- cache 验证按同仓 PR 与 fork PR 分离 | 选:同仓 PR 记录 cache hit；若 miss，则等待保存后在同 SHA rerun；fork 只验证只读恢复边界 | 弃:使用 `pull_request_target`(扩大执行权限)、清空现有 cache(破坏共享状态) | 主导:讨论收敛 | 依据:CI job log / GitHub cache 行为
- 已合入 evidence 后出现审查修复时，使用受限的新 evidence carrier | 选:创建独立 bounded review-remediation PR，仅承载 OpenSpec 与验证证据，并记录当前 branch 与 superseded branch | 弃:直接在已完成 evidence 后追加未验证修改、在 PR 合入后继续读取会变化的 live `merge_commit_sha`、追加递归 self-evidence commit | 主导:讨论收敛 | 依据:PR #9 / 最终 master gate
- 本 change 保持 workflow runtime 与供应链边界范围 | 选:只修改 CI/Security workflow、OpenSpec 与验证证据 | 弃:同时修改 Rust/Cargo 或引入 release workflow(超出本 change 范围) | 主导:用户 | 依据:proposal / spec / tasks

## 变更
- 将 CI 与 Security workflow 的 `actions/checkout`、`actions/cache` 升级到 Node.js 24 兼容版本并固定完整 commit SHA。
- 为 workflow 增加 `contents: read`、`persist-credentials: false` 与可核验的 tested revision marker。
- 完成同仓 PR、master、cache、权限边界、revision provenance 和 review-remediation 验证。
- 修复 merged PR API 中 `merge_commit_sha` 可变导致的证据重放问题，并通过 bounded review-remediation carrier 持久化修复。
- 完成 `dependency-security` 主 spec 同步；OpenSpec tasks 共 `21/21` 完成，strict validation `18/18` 通过。

## 待决
- v1.2.0 release automation 由后续独立 change 规划。
- Action pin 的周期性维护由后续 dependency maintenance change 处理。
- `bincode` unmaintained warning 保持在既有 dependency governance 范围，本 change 不处理。

## 引用
- OpenSpec change:`modernize-github-actions-runtime`;spec:`dependency-security`。
- GitHub:PR #7(implementation)、PR #8(初始 evidence)、PR #9(bounded review-remediation evidence carrier)。
- PR #9 branch:`codex/modernize-github-actions-runtime-review-fix`;head:`92ae74dff5ce51535003424d7168b239d9bc231e`;merge:`d6dd905e18b2f685f5ed5c739c106fe6d9adf7c6`。
- PR #8 carrier 已被 PR #9 carrier 取代；未追加递归 self-evidence commit。
- CI run:`29193587567`,attempt 1,event:`push`,conclusion:`success`。
- CI job:`86652471812`,`fmt · clippy · test · build (windows-latest)`,conclusion:`success`,marker:`d6dd905e18b2f685f5ed5c739c106fe6d9adf7c6`。
- CI job:`86652471817`,`fmt · clippy · test · build (ubuntu-latest)`,conclusion:`success`,marker:`d6dd905e18b2f685f5ed5c739c106fe6d9adf7c6`。
- Security run:`29193587560`,attempt 1,event:`push`,conclusion:`success`。
- Security job:`86652471799`,`RustSec dependency audit`,conclusion:`success`,marker:`d6dd905e18b2f685f5ed5c739c106fe6d9adf7c6`。
- runtime/cache warning matches:`0`;OpenSpec strict:`18/18`;tasks:`21/21`。
- 关联决策:`2026-07-12-61-archive-modernize-ratatui-and-deny-unsound.md`。
