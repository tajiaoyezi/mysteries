## ADDED Requirements

### Requirement: 提交的 Cargo.lock 接受 RustSec vulnerability 门禁

仓库 SHALL 以根目录中已跟踪、已提交、Git mode 为 `100644` 的 regular non-symlink `Cargo.lock` 为依赖安全事实源，并 MUST 通过显式 `--file Cargo.lock` 使用当前可获取的 RustSec advisory database 扫描其完整依赖图。根 lockfile 缺失、未跟踪、非 regular、symlink 或 mode 异常 MUST 在审计前失败，不得生成或跟随替代 lockfile。任一 vulnerability MUST 使审计失败；RustSec informational warning MUST 保持可见，但在本 capability 的首版 MUST NOT 阻断审计。成功结果 MUST 明确区分“0 vulnerability”与剩余 warning，不得宣称 warning-free。crates.io index 支撑的 yanked 检查为 best-effort，index 故障 MUST 输出 warning，但不属于本 capability 的 hard-fail 保证。

#### Scenario: 无 vulnerability 的 lockfile 通过审计

- **WHEN** 根 `Cargo.lock` 不匹配 RustSec database 中的任何 vulnerability，但可能匹配 allowed informational warning
- **THEN** 审计以成功状态结束，并在输出中保留每个 informational warning 的 advisory ID、crate、版本与依赖路径

#### Scenario: 任一 vulnerability 阻断审计

- **WHEN** 根 `Cargo.lock` 的任一直接或传递依赖匹配 RustSec vulnerability
- **THEN** 审计 MUST 以非零状态结束，并输出 advisory ID、受影响 crate / 版本、修复建议与反向依赖路径

#### Scenario: 缺失或未跟踪的根 lockfile 被拒绝

- **WHEN** 根 `Cargo.lock` 不存在、不是 regular file、是 symlink、未被 Git 跟踪或 Git mode 不是 `100644`
- **THEN** 安全门禁 MUST 在调用审计前失败，MUST NOT 通过 `cargo update`、`cargo generate-lockfile` 或其他方式生成替代 lockfile 后继续

#### Scenario: 当前三个 vulnerability 不得被例外掩盖

- **WHEN** 审计 `RUSTSEC-2026-0204`、`RUSTSEC-2026-0194` 或 `RUSTSEC-2026-0195`
- **THEN** 仓库 MUST 通过修复版本或移除受影响依赖路径使其消失，MUST NOT 为这些 ID 配置 ignore、patch、vendor 或输出过滤来制造成功结果

#### Scenario: 剩余 warning 如实报告

- **WHEN** 修复本 change 后的 lockfile 仍包含 `bincode 1.3.3`、`paste 1.0.15` 或 `lru 0.12.5` 的 informational advisory
- **THEN** 审计输出 MUST 保留这些 warning，结果可因没有 vulnerability 而成功，但报告 MUST NOT 写成“0 advisory”或“warning-free”

#### Scenario: crates.io yanked index 故障为显式 best-effort

- **WHEN** RustSec advisory database 与根 `Cargo.lock` 可正常读取且没有 vulnerability，但 crates.io index fetch/open 失败
- **THEN** 审计 MUST 输出 index / yanked 检查不完整的 warning，结果允许成功，报告 MUST NOT 声称已完整验证 yanked 状态

### Requirement: CI 持续运行最小权限的依赖安全审计

仓库 SHALL 提供独立 `security-audit` GitHub Actions workflow，在单个 Ubuntu job 中使用精确固定版本的 `cargo-audit` 和显式 `--file Cargo.lock` 扫描已跟踪的根 lockfile。workflow MUST 只有 `contents: read` 权限，checkout MUST NOT 持久化凭据；审计工具 MUST 安装在 runner temp 下隔离、初始无配置的 `CARGO_HOME` / install root，并 MUST 通过绝对 binary path 执行，MUST NOT 经可被仓库 Cargo alias shadow 的 `cargo audit` dispatch。workflow MUST 更新 RustSec advisory database，MUST NOT 使用 `continue-on-error`；工具安装、完整版本断言、database 更新、指定 lockfile 读取/解析或审计执行错误均 MUST fail-closed。每个 pull request 与每次 push 到 `master` 都 MUST 运行该 workflow，使其可作为稳定 required check；现有 Windows + Ubuntu build/test workflow MUST 保持独立。

#### Scenario: 每个 PR 和 master push 都触发门禁

- **WHEN** 任一 pull request 创建或更新，或任一提交 push 到 `master`
- **THEN** 独立 Ubuntu `security-audit` job MUST 运行一次并以绝对 `cargo-audit` binary 的 `audit --file <absolute-root-Cargo.lock>` 退出状态决定门禁结果，现有双平台 CI 仍按原触发条件运行

#### Scenario: 仓库 Cargo alias 不能替换审计器

- **WHEN** pull request 新增 `.cargo/config.toml` 并用 `[alias] audit` 伪造版本输出或成功退出
- **THEN** workflow MUST 仍通过 runner temp 中安装后的绝对 `cargo-audit` binary 完成版本断言和审计，仓库 alias MUST NOT 被调用或影响 gate 结果

#### Scenario: 定时审计捕获 database 漂移

- **WHEN** `Cargo.lock` 没有变化，但每周 schedule 到达且 RustSec database 新增匹配当前依赖的 vulnerability
- **THEN** workflow MUST 拉取新 database、重新扫描已提交的根 `Cargo.lock` 并失败

#### Scenario: 维护者可手动重跑审计

- **WHEN** 维护者通过 `workflow_dispatch` 启动安全审计
- **THEN** workflow MUST 使用与自动触发相同的固定工具版本、权限、database 更新和失败语义

#### Scenario: 审计基础设施错误不被吞掉

- **WHEN** `cargo-audit` 隔离安装失败、绝对 binary 完整版本输出不等于固定版本、RustSec database 无法更新、根 `Cargo.lock` 缺失/非 regular/symlink/未跟踪/mode 异常/无法解析、项目级 `.cargo/audit.toml` 存在，或隔离 `CARGO_HOME` 初始已含 `audit.toml` / `config.toml`
- **THEN** `security-audit` job MUST 失败，MUST NOT 以空结果、旧成功结果或 `continue-on-error` 继续

#### Scenario: 新增 Action 使用不可变引用

- **WHEN** `security-audit` workflow 引用 checkout 或任何其他 GitHub Action
- **THEN** 每个新增 Action MUST 固定到完整 commit SHA 并在邻近注释其可读 release tag，MUST NOT 只引用可移动 branch 或 major tag；checkout MUST 设置 `persist-credentials: false`

### Requirement: Advisory 例外默认禁止并接受精确治理

仓库的默认 advisory ignore set MUST 为空，首版 `security-audit` workflow MUST 拒绝项目级 `.cargo/audit.toml`，并 MUST 使用初始不含 `audit.toml` / `config.toml` 的隔离 `CARGO_HOME`。未来只有在已证明受影响路径不可达且无法及时升级或移除时，才可通过独立 change 同步修订本 spec 与 workflow 的严格配置校验，并添加精确 advisory ID；每个例外 MUST 记录依赖路径、不可达证据、上游链接、复查责任与移除条件。通配例外、无依据例外和为了让 CI 变绿而静默过滤输出 MUST 被禁止。

#### Scenario: 无例外时不创建伪配置

- **WHEN** 当前 vulnerability 均可通过升级或删除未使用依赖路径修复
- **THEN** 实现 MUST 保持 ignore set 为空、项目根不存在 `.cargo/audit.toml`、隔离 `CARGO_HOME` 初始无 `audit.toml` / `config.toml`，MUST NOT 添加无内容的例外、占位 ignore 或对当前 advisory 的临时豁免

#### Scenario: 合法例外必须可审计和可移除

- **WHEN** 未来变更提议忽略一个确实暂时不可修复且已证明不可达的 advisory
- **THEN** 独立 change MUST 先修订本 requirement 与 workflow 的 strict config validator；配置只能列精确 advisory ID，并在同一变更中记录依赖路径、证据、上游链接、复查责任及触发移除的修复版本或条件

#### Scenario: 不完整或宽泛例外被拒绝

- **WHEN** 例外使用通配、范围、无 rationale 的 ID，或缺少依赖路径、证据、复查责任、移除条件之一
- **THEN** 该例外 MUST NOT 被接受为符合本 capability 的实现
