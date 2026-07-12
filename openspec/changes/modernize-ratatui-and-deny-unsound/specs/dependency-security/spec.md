## MODIFIED Requirements

### Requirement: 提交的 Cargo.lock 接受 RustSec vulnerability 门禁

仓库 SHALL 以根目录中已跟踪、已提交、Git mode 为 `100644` 的 regular non-symlink `Cargo.lock` 为依赖安全事实源，并 MUST 通过显式 `--file Cargo.lock` 使用当前可获取的 RustSec advisory database 扫描其完整依赖图。根 lockfile 缺失、未跟踪、非 regular、symlink 或 mode 异常 MUST 在审计前失败，不得生成或跟随替代 lockfile。任一 vulnerability 及任一 kind=`unsound` 的 RustSec informational warning MUST 使审计失败，审计命令 MUST 显式使用 `--deny unsound`；其他 informational warning MUST 保持可见但不阻断。实际命令含 `--deny unsound`、完整扫描指定 lockfile 并 exit 0 SHALL 作为“0 vulnerability / 0 unsound”的机器证据；人类报告 MUST 将该结论与剩余 allowed warning 明确区分，不得宣称 warning-free，也不得要求原始 `cargo-audit` 输出其不会生成的显式零计数行。crates.io index 支撑的 yanked 检查为 best-effort，index 故障 MUST 输出 warning，但不属于本 capability 的 hard-fail 保证。

#### Scenario: 无 vulnerability 和 unsound 的 lockfile 通过审计

- **WHEN** 根 `Cargo.lock` 不匹配 RustSec database 中的任何 vulnerability 或 unsound advisory，但可能匹配 allowed unmaintained informational warning
- **THEN** `--deny unsound` 审计以成功状态结束；该命令与 exit 0 共同证明 0 vulnerability / 0 unsound，输出仍保留每个 allowed warning 的 advisory ID、crate、版本与依赖路径，报告不得把结果写成 warning-free

#### Scenario: 任一 vulnerability 阻断审计

- **WHEN** 根 `Cargo.lock` 的任一直接或传递依赖匹配 RustSec vulnerability
- **THEN** 审计 MUST 以非零状态结束，并输出 advisory ID、受影响 crate / 版本、修复建议与反向依赖路径

#### Scenario: 任一 unsound warning 阻断审计

- **WHEN** 根 `Cargo.lock` 没有 vulnerability，但任一直接或传递依赖匹配 kind=`unsound` 的 RustSec informational advisory
- **THEN** `--deny unsound` 审计 MUST 以非零状态结束并保留 advisory 与依赖路径，MUST NOT 因它被分类为 informational warning 而放行

#### Scenario: 缺失或未跟踪的根 lockfile 被拒绝

- **WHEN** 根 `Cargo.lock` 不存在、不是 regular file、是 symlink、未被 Git 跟踪或 Git mode 不是 `100644`
- **THEN** 安全门禁 MUST 在调用审计前失败，MUST NOT 通过 `cargo update`、`cargo generate-lockfile` 或其他方式生成替代 lockfile 后继续

#### Scenario: 当前三个 vulnerability 不得被例外掩盖

- **WHEN** 审计 `RUSTSEC-2026-0204`、`RUSTSEC-2026-0194` 或 `RUSTSEC-2026-0195`
- **THEN** 仓库 MUST 通过修复版本或移除受影响依赖路径使其消失，MUST NOT 为这些 ID 配置 ignore、patch、vendor 或输出过滤来制造成功结果

#### Scenario: 剩余 unmaintained warning 如实报告

- **WHEN** 本 change 后的 lockfile 仍包含 `syntect -> bincode 1.3.3` 的 `RUSTSEC-2025-0141` unmaintained warning，但不含 vulnerability 或 unsound warning
- **THEN** 审计输出 MUST 保留该 warning，结果可成功，但报告 MUST NOT 写成“0 advisory”或“warning-free”

#### Scenario: crates.io yanked index 故障为显式 best-effort

- **WHEN** RustSec advisory database 与根 `Cargo.lock` 可正常读取且没有 vulnerability / unsound，但 crates.io index fetch/open 失败
- **THEN** 审计 MUST 输出 index / yanked 检查不完整的 warning，结果允许成功，报告 MUST NOT 声称已完整验证 yanked 状态

### Requirement: CI 持续运行最小权限的依赖安全审计

仓库 SHALL 提供独立 `security-audit` GitHub Actions workflow，在单个 Ubuntu job 中使用精确固定版本的 `cargo-audit`，并以显式 `audit --deny unsound --file <absolute-root-Cargo.lock>` 扫描已跟踪的根 lockfile。workflow MUST 只有 `contents: read` 权限，checkout MUST NOT 持久化凭据；审计工具 MUST 安装在 runner temp 下隔离、初始无配置的 `CARGO_HOME` / install root，并 MUST 通过绝对 binary path 执行，MUST NOT 经可被仓库 Cargo alias shadow 的 `cargo audit` dispatch。workflow MUST 更新 RustSec advisory database，MUST NOT 使用 `continue-on-error`；vulnerability、unsound warning、工具安装、完整版本断言、database 更新、指定 lockfile 读取/解析或审计执行错误均 MUST fail-closed。每个 pull request 与每次 push 到 `master` 都 MUST 运行该 workflow，使其可作为稳定 required check；现有 Windows + Ubuntu build/test workflow MUST 保持独立。

#### Scenario: 每个 PR 和 master push 都触发门禁

- **WHEN** 任一 pull request 创建或更新，或任一提交 push 到 `master`
- **THEN** 独立 Ubuntu `security-audit` job MUST 运行一次，并以绝对 `cargo-audit` binary 的 `audit --deny unsound --file <absolute-root-Cargo.lock>` 退出状态决定门禁结果；现有双平台 CI 仍按原触发条件运行

#### Scenario: CI 遇到 unsound warning 时失败

- **WHEN** PR 的根 lockfile 没有 vulnerability，但包含任一 kind=`unsound` advisory
- **THEN** `security-audit` job MUST 因 `--deny unsound` 非零退出，MUST NOT 通过输出过滤、allowed warning 或 `continue-on-error` 继续

#### Scenario: 仓库 Cargo alias 不能替换审计器

- **WHEN** pull request 新增 `.cargo/config.toml` 并用 `[alias] audit` 伪造版本输出或成功退出
- **THEN** workflow MUST 仍通过 runner temp 中安装后的绝对 `cargo-audit` binary 完成版本断言和 `audit --deny unsound` 审计，仓库 alias MUST NOT 被调用或影响 gate 结果

#### Scenario: 定时审计捕获 database 漂移

- **WHEN** `Cargo.lock` 没有变化，但每周 schedule 到达且 RustSec database 新增匹配当前依赖的 vulnerability 或 unsound advisory
- **THEN** workflow MUST 拉取新 database、重新扫描已提交的根 `Cargo.lock` 并失败

#### Scenario: 维护者可手动重跑审计

- **WHEN** 维护者通过 `workflow_dispatch` 启动安全审计
- **THEN** workflow MUST 使用与自动触发相同的固定工具版本、`--deny unsound`、权限、database 更新和失败语义

#### Scenario: 审计基础设施错误不被吞掉

- **WHEN** `cargo-audit` 隔离安装失败、绝对 binary 完整版本输出不等于固定版本、RustSec database 无法更新、根 `Cargo.lock` 缺失/非 regular/symlink/未跟踪/mode 异常/无法解析、项目级 `.cargo/audit.toml` 存在，或隔离 `CARGO_HOME` 初始已含 `audit.toml` / `config.toml`
- **THEN** `security-audit` job MUST 失败，MUST NOT 以空结果、旧成功结果或 `continue-on-error` 继续

#### Scenario: 新增 Action 使用不可变引用

- **WHEN** `security-audit` workflow 引用 checkout 或任何其他 GitHub Action
- **THEN** 每个新增 Action MUST 固定到完整 commit SHA 并在邻近注释其可读 release tag，MUST NOT 只引用可移动 branch 或 major tag；checkout MUST 设置 `persist-credentials: false`
