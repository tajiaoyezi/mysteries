# 2026-07-11 · 60 · archive harden-dependency-security

## 决策
- 漏洞修复采用“精确升级真实使用依赖 + 删除未使用能力面” | 选:`crossbeam-epoch 0.9.20` + `syntect` 最小 `default-syntaxes` / `default-themes` / `regex-fancy` features | 弃:保留 `default-fancy` 只升级 `plist` / `quick-xml`(继续编译未使用 loader)、全量 `cargo update`(引入无关 churn) | 主导:讨论收敛 | 依据:code / dependency tree / RustSec
- RustSec CI 使用独立 Ubuntu workflow、固定 `cargo-audit 0.22.2`、runner temp 隔离 `CARGO_HOME` / install root 和绝对 binary path | 选:每个 PR、`master` push、每周 schedule、`workflow_dispatch` 都扫描已提交根 lockfile | 弃:`cargo audit` external-subcommand dispatch(可被仓库 Cargo alias shadow)、共享 runner Cargo home / advisory cache(配置污染或 freshness 风险)、并入双平台 build matrix(重复扫描) | 主导:安全审查收敛 | 依据:恶意 alias 负向验证 / CI
- 门禁语义为 vulnerability hard-fail、informational warning report-only、crates.io index/yanked best-effort | 选:0 vulnerability 时允许 `bincode` / `paste` / `lru` warning 保持可见 | 弃:advisory ignore / 输出过滤(制造假绿)、首版 `--deny warnings`(会把 `ratatui` 迁移扩入本 change) | 主导:讨论收敛 | 依据:RustSec 输出 / spec
- `Cargo.lock` 只接受已跟踪、Git mode=`100644` 的 regular non-symlink 根文件，checkout 不持久化凭据，新增 Action 固定完整 SHA | 弃:生成替代 lockfile 后继续、跟随 symlink、移动 tag、让后续供应链步骤保留 token | 主导:代码审查 | 依据:负向 preflight / workflow logs
- `syntect` feature 收窄不授权 runtime 或 UI 改动 | 选:仅补未闭合 Rust fence characterization，并要求既有暗/亮主题 `insta` 快照零 churn | 弃:修改渲染分支或 approve 新快照来适配依赖变化 | 主导:spec 约束 | 依据:tests / 真机验证
- 10.1–10.5 验收允许用户亲测或显式委托 agent，但每项必须基于真实本地、TUI 或远端证据 | 选:本地审计/依赖树由 agent 执行，TUI 由用户截图与退出确认，PR 与 post-merge workflow 由 agent 持续监控 | 弃:用自动快照冒充真机、用旧 SHA 的 green 替代最新 head | 主导:用户授权 | 依据:真机截图 / PR checks / workflow_dispatch

## 变更
- 将 `crossbeam-epoch` 从 `0.9.18` 精确更新到 `0.9.20`，移除 `plist`、`quick-xml`、`yaml-rust` 及 loader-only 传递依赖；RustSec 从 3 vulnerability / 4 warning 收敛为 0 vulnerability / 3 visible warning。
- 新增独立 `security-audit` workflow，包含 lockfile preflight、隔离安装、完整版本断言、绝对 binary 审计、最小权限和四类触发入口。
- 补未闭合 Rust fence characterization，更新 CONTRIBUTING、README、CHANGELOG 与 `dependency-security` / `tui-shell` specs。
- fmt、clippy、全量 test、release build、strict OpenSpec validation、零快照 churn、真机 Markdown、PR checks 与合入后 `workflow_dispatch` 全部通过；实现与最终验收记录分别经 PR #1 / #2 合入。

## 待决
- `actions/checkout@v4` / `actions/cache@v4` 的 Node.js 20 runtime 已由 GitHub runner 强制转为 Node.js 24，需后续独立 change 升级 Action 并重新固定完整 SHA。
- `ratatui` 升级、`lru` unsound / `paste` unmaintained、`bincode` unmaintained 与 `--deny unsound` / warning policy 继续作为独立依赖现代化 change。

## 引用
- OpenSpec change:`harden-dependency-security`;spec:`dependency-security`、`tui-shell`。
- GitHub:PR #1(实现)、PR #2(最终验收记录);`master` 上 post-merge `workflow_dispatch` 与最终 push checks。
- 关联决策:`parallelize-safe-tool-calls`(2026-07-11-59)。
