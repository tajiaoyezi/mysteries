## Why

仓库当前 package version 仍为 `1.1.0`，但 `CHANGELOG.md` 的 `Unreleased` 已积累并行安全工具、Network 权限级、Plan 持久化与供应链加固等可发布能力；与此同时远端没有任何 Git tag / GitHub Release，发布产物仍靠仓库内手工提交的 Windows executable，无法证明某个版本由哪份已验证源码可复现地产生。现在应把 release automation、版本固化与首次自动化 GitHub Release 合并为一个 change，以同一套不可变 revision、最小权限和远端门禁交付 v1.2.0。

## What Changes

- 新增 `.github/workflows/release.yml`：release-sensitive PR 与手动 dry-run 只执行双平台 package validation；仅严格匹配 package version 的 `v*` tag 可进入 publish path。
- 在固定 `windows-2022` / `ubuntu-22.04` runner 上以固定 Rust `1.96.1` 和 `--locked` 分别构建 `x86_64-pc-windows-msvc` / `x86_64-unknown-linux-gnu` release binary；Linux 明确以 glibc 2.35 为兼容 baseline并验证所需 GLIBC symbol version。两端验证 `--version` / `--help`，打包 LICENSE 与 README，并上传携带完整 target triple 的 artifact；聚合阶段生成并验证 `SHA256SUMS`。
- 所有第三方 Action 固定到官方 release tag 对应的完整 commit SHA并保留邻近 tag 注释；checkout 只使用内建只读 token且不持久化凭据。build/package jobs 仅 `contents: read`，唯一 publish job 才获得 `contents: write`，不得 checkout或获得 `id-token`、packages、pull-requests及其他写权限；`GH_TOKEN` 环境变量只绑定 Release API steps，公开后验证使用匿名 HTTPS asset URL。
- tag publish 必须来自已经合入 `master` 的精确 release commit；创建 tag 前先验证该 commit 的普通 CI 与 Security audit 全绿。workflow 再校验 canonical stable SemVer、annotated tag object、`Cargo.toml`、`Cargo.lock`、`mysteries --version` 与 Changelog版本完全一致；publish job按repository/tag串行，在首次 Release API 写操作前重新核验 `origin/master`，失败时不得创建 draft或公开 Release。
- publish job在所有前置门禁通过后创建 draft GitHub Release、上传并重新校验完整 asset set 与 checksums，全部成功后才转为公开；draft创建前失败不得产生Release，创建后的失败保留可诊断的非公开 draft，不自动删除/覆盖 tag 或既有 Release。
- 将 `Cargo.toml` / `Cargo.lock` package version 升级为 `1.2.0`，把 `Unreleased` 固化为带日期的 v1.2.0 release notes，并显式记录含 `SessionLine::Plan` 的 v1.2 session 回退到 v1.1.0 会解析失败这一降级不兼容边界。
- 纠正历史发布叙述：`1.0.0` / `1.1.0` 保留为开发里程碑但不补造历史 tag / GitHub Release；v1.2.0 定义为首个自动化、可复现的 GitHub Release。
- 更新 README / `deliverables/README.md` 的安装、下载、校验与产物定位说明；现有 v1.1.0 executable 可保留为历史验证产物，但 v1.2.0 及后续 release binary 不再提交进 Git。
- 本 change 的实施 PR 合入、精确 merge SHA 的 `master` CI/Security 成功、`v1.2.0` tag、release workflow 成功、公开 GitHub Release 与下载后双平台 smoke 均完成后才可 archive。
- **BREAKING（仅降级方向）**：v1.2.0 写出的含 `SessionLine::Plan` 会话文件不保证可被旧 v1.1.0 binary 读取；v1.2.0 对旧会话仍保持向后兼容。
- 明确不在本 change：macOS / ARM artifacts、crates.io 发布、代码签名、SBOM、SLSA/attestation、历史 tag 回填、Git 历史重写、运行时功能或 TUI 布局/样式改动；package version 升级导致的既有 TUI 版本文本快照基线 `v1.1.0` → `v1.2.0` 更新除外。

## Capabilities

### New Capabilities
- `release-delivery`：定义 release-sensitive PR 验证、tag/version/revision 一致性、Windows/Linux package、checksums、draft-to-public GitHub Release、下载后 smoke 与归档门禁。

### Modified Capabilities
- `dependency-security`：允许仅在 tag publish job 中按最小范围授予 `contents: write`，并把 release workflow 纳入不可变 Action、受支持 runtime、checkout credential 与 fail-closed 供应链边界。

## Impact

- **CI / 发布**：新增 `.github/workflows/release.yml`，并在 PR、`master` merge、tag 与 GitHub Release 之间建立不可跳过的 revision provenance；现有 `ci.yml` 与 `security-audit.yml` 的触发、job 名称和权限不变。
- **版本 / 锁文件**：只修改根 package 的 `Cargo.toml` / `Cargo.lock` version，不升级 dependency、不新增 Rust crate、不改变 MSRV。
- **文档**：更新 `CHANGELOG.md`、README 与 `deliverables/README.md`；历史 v1.1 executable 不作为 v1.2 release source。
- **规格**：新增 `release-delivery` delta，修订 `dependency-security` 的 publish-job 最小权限规则。
- **兼容性**：Agent Loop、Provider、Tool、Permission、Config、Session wire implementation、TUI 布局/样式与 CLI flag grammar 均不改变；`--version` 和既有 TUI header 版本文本通过现有 `CARGO_PKG_VERSION` 自动显示 `1.2.0`，只更新对应 snapshot baseline 的版本字面量。
- **依赖**：workflow 可使用 GitHub-hosted runner 预装 shell / `gh` 与固定 SHA 的官方 artifact Actions；不得引入第三方 release Action 或 Agent SDK。
