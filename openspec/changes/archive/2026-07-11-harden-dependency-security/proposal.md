## Why

当前提交的 `Cargo.lock` 被 `cargo audit` 检出 3 个 RustSec vulnerability：`crossbeam-epoch 0.9.18` 1 个、未使用的 `syntect -> plist -> quick-xml 0.39.4` 依赖链 2 个；现有 CI 只验证格式、编译、测试与 release build，无法阻止已知漏洞或后续新增 advisory 进入主分支。现在应先以最小依赖面修复真实漏洞，再建立可持续、最小权限且默认 fail-closed 的 RustSec 门禁。

## What Changes

- 把 `crossbeam-epoch` 的 lockfile 解析版本更新至已修复的 `0.9.20`，不升级 `ignore` 或扩大无关传递依赖 churn。
- 将 `syntect` 从聚合 feature `default-fancy` 收窄为项目实际需要的 `default-syntaxes`、`default-themes`、`regex-fancy`，保留内置 syntax/theme 与纯 Rust 语法高亮，同时从依赖图移除未使用的 `plist-load`、`yaml-load`、`html` 及 `plist -> quick-xml` / `yaml-rust` 路径。
- 为既有“半截 markdown 流式不 panic”契约补一个 test-only characterization case，锁定未闭合 Rust fence 在最小 `syntect` features 下仍尽力渲染；不修改 runtime 逻辑或快照基线。
- 新增独立的 Ubuntu `security-audit` GitHub Actions workflow：checkout 不持久化凭据，在 runner temp 的隔离 `CARGO_HOME` / install root 中安装固定版本 `cargo-audit`，并通过绝对 binary path 审计仓库已跟踪的根 `Cargo.lock`；不得经可被仓库 Cargo alias shadow 的 `cargo audit` dispatch。在每个 pull request、push 到 `master`、每周定时及手动触发时执行，使其可作为稳定 required check。
- 任何 vulnerability、审计工具安装/版本断言错误、RustSec advisory database fetch/load 失败或指定 lockfile 读取/解析失败都阻断门禁；RustSec informational warning 保持可见但本 change 不用 `--deny warnings` 阻断，crates.io index/yanked 检查明确为 best-effort。
- 首版禁止项目级 `.cargo/audit.toml`，并要求 runner temp 下的隔离 `CARGO_HOME` 初始不含 `audit.toml` / `config.toml`，封死降低 database freshness、severity、target coverage、warning visibility 或通过 Cargo alias 替换审计程序的配置绕过；尤其不得忽略当前 3 个 vulnerability。未来例外必须通过独立 change 修订门禁，并记录精确 advisory ID、依赖路径、依据与移除条件。
- 同步 `CONTRIBUTING.md`、README 工程质量段与 `CHANGELOG.md` Unreleased，使贡献者本地命令、公开 CI 说明和发布记录反映新增安全门禁。
- 明确不在本 change：`ratatui 0.30` 升级、全部 warning 清零、`cargo-deny`、SBOM、license/bans/sources policy、Dependabot、MSRV 契约调整，以及任何运行时功能或 UI 改动。

## Capabilities

### New Capabilities

- `dependency-security`：规定提交的 Rust 依赖锁文件如何接受 RustSec 审计、CI 如何阻断 vulnerability，以及 advisory 例外如何治理。

### Modified Capabilities

- `tui-shell`：将 markdown 语法高亮依赖从 `syntect(default-fancy)` 聚合 feature 改为显式的最小 feature 契约，同时保持现有暗/亮主题、默认 syntax/theme、未知语言 fallback 与渲染结果不变。

## Impact

- **依赖 / 测试文件**：`Cargo.toml` 的 `syntect` features、`Cargo.lock` 的传递依赖图，以及 `src/tui/markdown.rs` 的 test-only 未闭合 fence characterization；不新增 runtime crate，不修改 package API 或运行时逻辑。
- **CI**：新增 `.github/workflows/security-audit.yml`；现有 `.github/workflows/ci.yml` 的 Windows + Ubuntu build/test matrix 与触发条件不变。
- **文档**：更新 `CONTRIBUTING.md` 的提交前安全命令、README 的独立 security workflow 说明，以及 `CHANGELOG.md` Unreleased 安全条目。
- **规格**：新增 `dependency-security` 主能力 delta，并修订 `tui-shell` 中对 `default-fancy` 的实现级约束。
- **兼容性**：不改变 CLI、配置、session wire、Agent Loop、工具权限、TUI 布局或用户可见行为；不声明新的 MSRV。依赖 feature 收窄必须由既有 markdown 单测、新增 test-only 未闭合 fence case 及全部 `insta` 快照零 churn 证明。
- **UI / 设计规范**：本 change 不改变任何视觉、布局或交互语义，故 `设计规范/` 的 port / adapt / drop 分类不适用；既有快照不得因本 change 被 approve 或改写。
- **验证**：先保存当前直接 `cargo-audit audit --file Cargo.lock` RED 基线和 `cargo tree --locked -i` 反向依赖路径，再验证 vulnerability 清零、未使用依赖消失、informational warning 明确报告，以及 fmt、clippy、全量 test、release build、strict OpenSpec validation 全部通过。
