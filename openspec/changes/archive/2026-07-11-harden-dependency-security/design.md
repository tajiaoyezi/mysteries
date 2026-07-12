## Context

仓库根 `Cargo.lock` 当前包含 284 个 crate dependency。以 `cargo-audit 0.22.1` 和 2026-07-11 拉取的 RustSec advisory database 扫描，基线为 3 个 vulnerability、4 个 allowed informational warning：

| 分类 | Advisory | 当前依赖路径 | 本 change 处理 |
|---|---|---|---|
| vulnerability | `RUSTSEC-2026-0204` | `ignore 0.4.26 -> crossbeam-deque 0.8.6 -> crossbeam-epoch 0.9.18` | 将 `crossbeam-epoch` 精确更新至 `0.9.20` |
| vulnerability | `RUSTSEC-2026-0194`、`RUSTSEC-2026-0195` | `syntect 5.3.0 -> plist 1.9.0 -> quick-xml 0.39.4` | 移除项目未使用的 `plist-load` feature 路径 |
| unmaintained | `RUSTSEC-2025-0141` | `syntect 5.3.0 -> bincode 1.3.3` | 保持可见，后续治理 |
| unmaintained | `RUSTSEC-2024-0436` | `ratatui 0.29.0 -> paste 1.0.15` | 保持可见，后续 `ratatui` 升级治理 |
| unmaintained | `RUSTSEC-2024-0320` | `syntect 5.3.0 -> yaml-rust 0.4.5` | 随未使用的 `yaml-load` feature 移除 |
| unsound | `RUSTSEC-2026-0002` | `ratatui 0.29.0 -> lru 0.12.5` | 保持可见，后续 `ratatui` 升级治理 |

当前 `.github/workflows/ci.yml` 在 Windows 与 Ubuntu 上执行 fmt、clippy、全量 test 和 release build，但没有依赖安全门禁。RustSec database 可能在 `Cargo.lock` 没有变化时新增 advisory，因此仅在依赖 PR 中人工运行审计不足。

代码只调用 `SyntaxSet::load_defaults_newlines`、`ThemeSet::load_defaults` 和 `HighlightLines`；没有调用 plist/yaml/html loader。`syntect 5.3.0` 的 `default-fancy` 是聚合 feature，额外启用了 `html`、`plist-load`、`yaml-load` 等当前运行时不需要的能力。

本 change 横跨 manifest、lockfile、TUI 既有依赖契约与 GitHub Actions，且属于供应链安全变更，因此需要显式设计和可回滚迁移。它不新增 headless 内核行为，AGENTS.md 的强制 RED→GREEN TDD 停点不适用；旧 lockfile 的失败审计是安全基线证据，不是新接口测试。

## Goals / Non-Goals

**Goals:**

- 让当前 3 个 RustSec vulnerability 从提交的根 `Cargo.lock` 中消失，且不使用 ignore、patch 或 vendor 掩盖。
- 删除项目未使用的 `syntect` loader 依赖面，同时保持默认 syntax/theme、纯 Rust `regex-fancy` 与现有 markdown 渲染结果。
- 用 test-only characterization 补齐既有未闭合 fence 场景的自动化证据，不修改 markdown runtime 行为。
- 建立独立、最小权限、固定审计工具版本的 CI gate；新 vulnerability，以及工具安装/版本、RustSec database、lockfile 与审计执行错误均 fail-closed；crates.io index/yanked 检查按工具既有能力显式保留为 best-effort。
- 让剩余 informational warning 在本地和 CI 输出中可见，并如实声明 change 完成后仍非 warning-free。
- 保持现有 Windows + Ubuntu build/test CI、运行时功能、CLI、TUI 布局、交互与所有既有快照不变。

**Non-Goals:**

- 不在本 change 升级 `ratatui 0.29`，不处理其 `paste` / `lru 0.12.5` 路径。
- 不替换 `syntect`，不为消除 `bincode 1.3.3` 重做内置 syntax/theme 资源加载。
- 不启用 `--deny warnings`、`--deny unsound` 或 warning allowlist；warning 阻断策略留给后续依赖现代化 change。
- 不引入 `cargo-deny`、SBOM、license/bans/sources policy、Dependabot 或自动修复 bot。
- 不声明或改变项目 MSRV；仓库继续由 `rust-toolchain.toml` 的 `stable` 约束。
- 不修改 Agent Loop、Provider、工具、权限、session、配置或任何用户可见 UI 行为。

## Decisions

### 1. 收窄 `syntect` features，而不是升级未使用的 `plist`

`Cargo.toml` 将使用：

```toml
syntect = {
  version = "5",
  default-features = false,
  features = ["default-syntaxes", "default-themes", "regex-fancy"]
}
```

三项 feature 分别保留当前代码实际使用的内置 syntax dump、内置 theme dump 与纯 Rust regex engine。移除 `default-fancy` 聚合 feature 后，feature `html`、`plist-load`、`yaml-load` 被关闭；crate `plist`、`quick-xml`、`yaml-rust` 以及只服务这些路径的 `linked-hash-map`、`time`、`time-core`、`deranged`、`num-conv`、`powerfmt` 预计从根 lockfile 消失。`bincode` 仍由内置 dump load 路径引入，这是预期剩余 warning；`base64`、`indexmap`、`serde_json` 等仍可能由其他依赖保留，不得把关闭 feature 误报成同名 crate 全部移除。

备选方案是保持 `default-fancy`，仅将 `plist 1.9.0 -> 1.10.0`、`quick-xml 0.39.4 -> 0.41.0`。该方案能机械修复漏洞，但继续编译项目从未调用的 loader，并保留 `yaml-rust` warning；因此选择最小能力面而非最小 manifest diff。

主 `tui-shell` spec 当前显式钉死 `default-fancy`，必须同步改为“默认 syntax/theme + `regex-fancy`、禁用未使用 loaders”的行为契约。该修改不授权 UI churn；所有 markdown 定向测试和既有 `insta` 快照必须保持原结果。

主 spec 还包含“未闭合 Rust fence 在流式中途尽力渲染且不 panic”的既有 scenario，但当前 `src/tui/markdown.rs` 单测与两份 markdown 快照都只覆盖闭合 fence。实施时 SHALL 只在该文件的 `#[cfg(test)]` 区域补 characterization：输入 `` ```rust\nfn ``，断言调用不 panic、已到达的 `fn` 仍出现在代码块输出中；这属于 TUI 事后回归证据，不授权 runtime 分支或快照变化。

### 2. 只精确更新受影响的 `crossbeam-epoch`

执行 `cargo update -p crossbeam-epoch --precise 0.9.20`。`crossbeam-deque 0.8.6` 的 semver 约束允许该修复版，因此无需升级直接依赖 `ignore`，也不应使用递归或全量 `cargo update` 带入无关版本变化。

验收同时检查 `Cargo.lock` 和反向依赖树，证明解析版本为 `>=0.9.20`，并审查 lockfile diff 只包含 feature 收窄与这一精确更新所需的增删。

### 3. 新增独立 `security-audit` workflow

新建 `.github/workflows/security-audit.yml`，而不是把审计复制进现有双平台 matrix：RustSec 扫描的是同一份提交的 `Cargo.lock`，在 Windows 和 Ubuntu 重复不会增加覆盖。

workflow 契约：

- job 仅运行于 `ubuntu-latest`，设置 `permissions: contents: read` 与 `timeout-minutes: 15`。
- 每个 `pull_request` 及每次 `push` 到 `master` 都触发，不使用 workflow-level paths filter，使该 job 能稳定配置为 required check，并让普通代码 PR 也用当时最新 database 复核现有 lockfile。
- 每周 schedule 无条件运行，以捕获 lockfile 不变但 RustSec 新增 advisory 的情况；同时提供 `workflow_dispatch`。
- checkout action 固定为完整 commit SHA（规划时确认 `actions/checkout@08eba0b27e820071cde6df949e0beb9ba4906955 # v4.3.0`），并设置 `persist-credentials: false`，避免可移动 tag 或把 read token 留给后续供应链步骤。
- 在 workspace preflight 中依次断言 `Cargo.lock` 是 regular non-symlink file（`test -f` 且 `test ! -L`）、可由 `git ls-files --error-unmatch -- Cargo.lock` 证明已跟踪，并由 `git ls-files -s` 证明 Git mode 精确为 `100644`；同时拒绝项目根 `.cargo/audit.toml`。
- 将 `CARGO_HOME` 设为 `$RUNNER_TEMP/cargo-audit-home`、install root 设为 `$RUNNER_TEMP/cargo-audit-root`，二者必须是本 job 新建的隔离目录；从 `$RUNNER_TEMP` 执行 `cargo install cargo-audit --version "$CARGO_AUDIT_VERSION" --locked --root "$AUDIT_ROOT"`，不读取 runner 默认 Cargo home 的 config / metadata。
- 定义 `AUDIT_BIN="$AUDIT_ROOT/bin/cargo-audit"`，用绝对路径执行 `test "$("$AUDIT_BIN" --version)" = "cargo-audit $CARGO_AUDIT_VERSION"`，再执行 `"$AUDIT_BIN" audit --file "$GITHUB_WORKSPACE/Cargo.lock"`。MUST NOT 经 `cargo audit` external-subcommand dispatch，避免 PR 中 `.cargo/config.toml` 的 `[alias] audit` 同时伪造版本与审计结果。
- 不得使用 `continue-on-error`；工具安装、绝对 binary 版本断言、advisory database fetch/load、指定 lockfile 读取/解析及 vulnerability 发现产生的非零退出都使 job 失败。
- `cargo-audit 0.22.2` 对 crates.io index fetch/open 失败只告警并跳过 yanked 检查，因此 yanked 状态明确为 best-effort，不纳入本 change 的 fail-closed 承诺；RustSec database 中的 vulnerability 与 informational advisory 仍按上述规则处理。
- 首版不缓存 advisory database 或项目 `target`，优先保证 database 新鲜和机制简单。若后续安装耗时不可接受，只能另行设计按 OS、架构和精确版本键控的审计 binary cache。

RustSec 的 `audit-check` action 是官方 README 推荐的 GitHub 集成，但其封装会自行发现/安装审计工具，并需要更宽的 Check/Issue 权限才能发挥完整能力。直接安装固定版本 `cargo-audit` 的首次运行较慢，但版本、权限与退出码语义更直接、可复现，也能稳定支持 fork PR，因此本 change 选择 CLI 方式。`cargo-deny` 能覆盖更广的 dependency policy，但需要额外配置和治理决策，超出当前 vulnerability gate。

### 4. Vulnerability hard-fail，informational warning report-only

CI 使用绝对 `cargo-audit` binary 的 `audit --file "$GITHUB_WORKSPACE/Cargo.lock"` 退出语义：任一 vulnerability 导致非零退出；RustSec database 中的 `unmaintained`、`unsound` 等 informational warning 输出但不阻断。yanked 检查依赖 best-effort crates.io index，不承诺 index 故障时仍完整。不得为 `RUSTSEC-2026-0204`、`RUSTSEC-2026-0194`、`RUSTSEC-2026-0195` 添加 ignore，也不为当前剩余 warning 添加 ignore 来制造“干净”输出。

该边界意味着本 change 完成后预计报告 3 个 allowed warning：`bincode 1.3.3`、`paste 1.0.15`、`lru 0.12.5`。其中 `lru` 是 `unsound`，必须在最终报告中显式保留为后续安全债，不能把“0 vulnerability”表述成“0 advisory”或“warning-free”。

首版 workflow 必须在发现项目级 `.cargo/audit.toml` 时失败，并使用本 job 新建且初始无 `audit.toml` / `config.toml` 的隔离 `CARGO_HOME`，防止配置通过 `fetch=false`、`stale=true`、`severity_threshold`、target filter、quiet、warning 配置、ignore 或 Cargo alias 削弱门禁。未来若确有不可达 vulnerability 需要例外，必须另开 change 同步修订 `dependency-security` spec 和 workflow 的严格配置校验；届时仍只允许精确 advisory ID，并记录依赖路径、不可达证据、上游链接、负责人/复查日期和移除条件。通配、静默 ignore 或为了让 CI 变绿而忽略均禁止。

### 5. 验证以依赖图、审计结果和零行为 churn 三类证据闭环

- **依赖证据**：保存实施前 `cargo-audit audit --file Cargo.lock` 与 `cargo tree --locked -i`；实施后证明 `crossbeam-epoch >=0.9.20`，且 `plist`、`quick-xml`、`yaml-rust` 不再存在。
- **安全证据**：本地直接 `cargo-audit audit --file Cargo.lock` exit code 为 0、没有 vulnerability 条目，并逐项报告剩余 allowed warning；workflow 另以隔离副本验证 missing / untracked / symlink / Git mode 异常的 `Cargo.lock`、项目级 `.cargo/audit.toml`、绝对 binary 版本不匹配，以及恶意 `[alias] audit` 都不能伪造成功。
- **行为证据**：运行 markdown 定向单测以及全量 fmt、clippy、test、release build；全部既有 `.snap` 零 diff、仓库无 `.snap.new`。不得 approve 快照来掩盖 feature 收窄回归。
- **阶段证据**：自动化 tasks 完成只代表 local apply ready；10.1–10.3 通过后才允许提交/推送，PR `security-audit` 10.4 通过后才允许 merge，合入后 `workflow_dispatch` 10.5 通过后才允许 archive。五项均在 `tasks.md` §10 保留唯一 checkbox；用户可亲自执行，也可显式委托实施 agent 依据真实命令、真机 UI 或远端 job 证据完成并勾选，未实际完成时不得代勾。

## Risks / Trade-offs

- **[收窄 feature 可能意外缺少 runtime 资源]** → 在改 manifest 后先编译并运行 markdown 默认 syntax/theme、暗/亮主题、未知语言 fallback 定向测试；任何快照变化均视为回归，不更新基线。
- **[`cargo-audit` 固定版本可能落后]** → 工具版本固定保证可复现，advisory database 每次更新保证新 advisory 可见；升级工具本身通过后续显式依赖维护变更完成。
- **[每个 PR 在隔离目录从源码安装审计工具增加 CI 时延]** → 接受稳定 required check、避免 Cargo alias/config 污染和可复现工具版本带来的时延；观察到真实瓶颈后再设计精确键控 binary cache，不用 paths filter 或共享 runner Cargo home 牺牲门禁稳定性。
- **[informational warning 不阻断，尤其现存 `lru` unsound]** → 输出保持可见并在验收报告中逐项登记；`ratatui` 升级与 `--deny unsound` 作为后续 change，不在本次安全补丁中冒险引入 TUI breaking migration。
- **[工具安装或 RustSec advisory DB 网络故障造成 CI 红灯]** → 这是有意的 fail-closed 取舍；不设置重试吞错或 `continue-on-error`，需要时由维护者重跑 job。crates.io index/yanked 网络故障只产生显式 best-effort warning，不写成同等 hard-fail。
- **[仅 Ubuntu 审计遗漏平台差异]** → 当前目标是对完整 committed lockfile 做 RustSec advisory 匹配，不做 target-specific dependency policy；现有 Windows + Ubuntu build/test matrix 继续承担平台兼容性。

## Migration Plan

1. 保存旧 lockfile 的直接 `cargo-audit audit --file Cargo.lock` RED 输出及相关 `cargo tree --locked -i` 路径，作为修复前证据；补齐未闭合 Rust fence test-only characterization。
2. 收窄 `syntect` features，精确更新 `crossbeam-epoch`，审查 `Cargo.toml` / `Cargo.lock` diff。
3. 先跑依赖图、markdown 定向回归与审计，再跑全量本地质量门禁；确认快照零 churn。
4. 新增独立安全 workflow，并验证隔离安装、绝对 binary、恶意 Cargo alias、checkout 凭据、事件、权限与失败语义。
5. 同步 CONTRIBUTING / README / CHANGELOG，完成 local 自动化门禁后交给用户依次执行 tasks §10：本地真机 → PR job → post-merge dispatch；最后才进入 archive。

回滚应整体 revert manifest、lockfile 和 workflow 变更。由于旧 lockfile 已知含 vulnerability，回滚只允许用于临时诊断，不能作为可发布或可合并的最终状态；若 feature 收窄确实破坏行为，应改用 proposal 中的 `plist 1.10.0` 备选修复并重新通过全部门禁，而不是恢复有漏洞版本。

## Open Questions

- 无阻塞问题。后续应单独规划 `ratatui 0.30` 迁移与 `--deny unsound` policy，再评估 `bincode` unmaintained 路径；这些不改变本 change 已批准的边界。
