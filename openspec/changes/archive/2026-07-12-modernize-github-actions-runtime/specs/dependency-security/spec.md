## ADDED Requirements

### Requirement: 现有 GitHub Actions workflow 使用不可变且受支持的供应链边界

仓库现有双平台 build/test CI 与独立 `security-audit` workflow SHALL 将每个外部 GitHub Action 固定到经官方 release tag 映射核验的完整 40 位 commit SHA，并 MUST 在引用邻近位置保留可读 release tag 注释；MUST NOT 使用可移动 branch、major tag 或 floating tag。JavaScript Action MUST 声明 GitHub-hosted runner 当前受支持的 runtime，workflow MUST NOT 依赖 runner 对已弃用 Node.js runtime 的强制 compatibility fallback，也 MUST NOT 通过 `ACTIONS_ALLOW_USE_UNSECURE_NODE_VERSION` 或等价开关掩盖弃用。两个 workflow MUST 显式限制为 `contents: read`，所有 checkout MUST 设置 `persist-credentials: false`；未来确需写权限的 publish job MUST 由独立 change 另行规定，不得预先扩大现有 CI 权限。

两个 workflow MUST 在 checkout 后以同名 `Show tested revision` step 执行 `echo "TESTED_REVISION=$(git rev-parse HEAD)"`，通过固定 marker 明确输出 runner 实际测试的 revision；该 step MUST 只读、不得新增权限或改变后续命令。本次 runtime 迁移除该 evidence step 外 MUST 保持 CI 的触发条件、job/check 名称、Windows/Linux matrix、cache paths/key、fmt、clippy、全量 test 与 release build 命令不变；MUST 保持 `security-audit` 的 job/check 名称、PR/`master`/schedule/`workflow_dispatch` 触发、固定 `cargo-audit` 版本、隔离安装、输入校验、绝对 binary path、`--deny unsound`、fail-closed 与 allowed warning 可见性不变。同仓 implementation PR 的 Action major 升级后首次 cache miss MAY 被接受，但后续 MUST 以相同 tested revision rerun 证明 save/restore 正常；外部 fork PR 获得 read-only cache token 时 MAY 只 restore，并 MAY 以显式 save-denied warning 继续成功，MUST NOT 为获得 cache 写权限改用 `pull_request_target`。

#### Scenario: 所有现有 Action 使用官方 tag 对应的不可变引用

- **WHEN** 静态检查 `.github/workflows/ci.yml` 与 `.github/workflows/security-audit.yml` 中的每个外部 `uses:`
- **THEN** 每个引用均为完整 40 位 commit SHA，邻近注释标明经官方 repository 核验的 release tag，不存在 `@vN`、branch 或其他可移动 ref

#### Scenario: JavaScript Action 不依赖弃用 runtime fallback

- **WHEN** 最终 PR head/base 二元组触发 `pull_request`，GitHub 为其生成 merge-ref `GITHUB_SHA`，且合入后精确 implementation merge commit 分别运行 Windows CI、Ubuntu CI 与 RustSec dependency audit
- **THEN** PR evidence MUST 记录 head SHA、base SHA、PR API merge SHA、三个 `Show tested revision` 输出、run IDs 与 attempts；两个 PR workflow 的 REST `run.head_sha` MUST 等于 PR head，三个 jobs 的 revision 输出 MUST 等于同一 tested merge-ref SHA，且该 merge commit 的 first/second parents MUST 分别等于记录的 base/head；PR merge-ref 与 implementation merge commit 上的三个 jobs 均成功，日志和 annotations 不含 Node.js 20 deprecated、被强制运行于 Node.js 24、`DEP0040 punycode` 或 `DEP0169 url.parse()` 语义的 runtime warning，也未设置允许不安全 Node.js runtime 的规避开关

#### Scenario: workflow 权限与 checkout 凭据保持最小化

- **WHEN** GitHub 为普通 CI 或 `security-audit` 创建 job token 并执行 checkout
- **THEN** workflow 的显式权限仅为 `contents: read`，checkout 不把凭据持久化到 repository config，后续 Cargo 或审计步骤不获得 contents write、pull-request write、OIDC 或 package write 权限

#### Scenario: 双平台 CI 行为在 runtime 迁移后保持不变

- **WHEN** pull request 触发普通 CI
- **THEN** 原有 Windows/Ubuntu matrix 各自使用未改变的 cache paths/key，并依次通过 fmt check、clippy deny warnings、包含 integration tests 的全量 `cargo test --locked` 与 `cargo build --release --locked`；Action 升级不得删除、跳过、放宽或重命名这些验证语义

#### Scenario: 依赖安全门禁在 runtime 迁移后保持不变

- **WHEN** pull request、`master` push、每周 schedule 或 `workflow_dispatch` 触发 `security-audit`
- **THEN** workflow 继续以固定 `cargo-audit`、隔离 install root、严格 input validation、绝对 binary path 和 `audit --deny unsound --file <absolute-root-Cargo.lock>` fail-closed；0 vulnerability / 0 unsound 时成功并继续展示既有 allowed unmaintained warning，不得宣称 warning-free

#### Scenario: 同仓 implementation PR 的 cache major 升级允许一次冷启动

- **WHEN** 来自同一 repository 分支的 implementation PR 使用新 Action SHA 首次运行而没有可恢复的旧 cache entry
- **THEN** 该次 cache miss 可继续执行完整 CI，cache paths/key 保持原值并在同一 job/attempt 结束时成功保存；随后 rerun 同一 workflow run 时，两个平台 MUST 保持同一 tested revision、OS 与 lockfile key，并成功 restore/hit 对应 cache，MUST NOT 因升级而静默禁用 cache

#### Scenario: 外部 fork PR 的 read-only cache 不被误判为 runtime 回归

- **WHEN** 外部 fork PR 被 GitHub 发放 read-only cache token，且 cache miss 后 post-job save 被拒绝
- **THEN** workflow MAY 输出明确 save-denied warning 并继续成功，restore 仍可使用；该 warning MUST 与 Node.js runtime、`punycode`、`url.parse()` deprecation 区分，workflow MUST NOT 改用 `pull_request_target` 或扩大 token 权限来强制写 cache

#### Scenario: 远端证据可持久追溯且不自我证明

- **WHEN** implementation PR、implementation merge 与 post-merge evidence PR 依次完成，并准备 archive
- **THEN** change 内 `manual-verification.md` MUST 持久记录迁移前基线、official tag/SHA、PR head/base/API merge SHA/tested merge-ref SHA及其 parents、run IDs/attempts、cache 结果、implementation merge SHA 及其 `master` push runs；post-merge evidence commit MUST 只证明更早的 implementation revisions，archive 前 MUST 唯一查询 evidence merge SHA 的 `push` CI/Security runs 并把 SHA、run IDs/attempts 与结论写入经用户审阅的 archive 决策记录，MUST NOT 追加递归 evidence commit 来证明自身
