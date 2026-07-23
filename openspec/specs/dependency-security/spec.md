# dependency-security Specification

## Purpose
定义仓库依赖安全事实源、RustSec vulnerability 门禁、最小权限 CI 审计与 advisory 例外治理规则。
## Requirements
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

### Requirement: Release workflow 将写权限隔离在 tag publish job

`.github/workflows/release.yml` SHALL 默认仅授予 `contents: read`。Windows/Linux version/package/build、artifact upload、download verification 与公开后 smoke jobs MUST 保持只读；只有同时满足 `push` event、canonical stable SemVer annotated tag、`github.run_attempt == 1`、版本一致性与全部 build dependencies 成功的 publish job MAY 在 job level 授予`contents: write`与`actions: read`，任何tag workflow rerun的publish job MUST在environment gate与写token前skip。首attempt publish job MUST NOT 获得`id-token`、`attestations`、`packages`、`pull-requests`、`actions: write`、`checks`、`issues`、`administration`或其他写权限。`actions: read` MUST只用于在首次Release API写入前读取当前tag run server-side `created_at`，并按workflow file/ID、event、head SHA与branch定位同一revision的master CI、Security audit及release dry-run，再按每个run的`run_attempt`读取attempt-specific jobs；run-level `status`/`conclusion`不得用于过滤历史attempt。只有全部精确预期jobs的server-side `completed_at`严格早于tag run创建时间的attempt可被按降序tuple确定性选中并记录精确`run_id/run_attempt`，tag后补跑不得成为证据或覆盖较早有效attempt。`contents: write`中的read能力 MAY仅在publish API step调用当前repository的Git ref/Compare endpoints验证candidate到remote master的ancestry，并用于既有Release写操作；该权限不得读取或操作其他repository、修改run、下载不相关artifact或替代tag/candidate provenance。

publish job MUST绑定预先由已获独立授权的admin配置的受保护`release` environment：reviewer精确为`tajiaoyezi`、`prevent_self_review=false`、`custom_branch_policies=true`且唯一policy由admin以`name=v<version>,type=tag`创建，并禁止admin bypass；publish必须通过`/actions/runs/{run_id}/approvals`证明存在`state=approved`、`user.login=tajiaoyezi`且绑定当前`release` environment精确ID/name的review，并拒绝同environment的任何非approved review，而不是仅检查用户名或依赖bypass。runtime还必须分页读取deployment-branch-policies并要求仅有精确`v<version>` name，但不得依赖只读响应schema未承诺的policy `type`字段；`type=tag`由tag前admin证据承担。非publish checkout MAY仅通过官方`actions/checkout`默认token input使用内建只读`github.token`，但 MUST设置`persist-credentials: false`；除此之外，validation/build/package/checksum/public smoke steps MUST NOT把`github.token`显式传作Action input，也不得设置`GH_TOKEN`/`GITHUB_TOKEN`环境变量。

publish job MUST NOT checkout或执行任何checkout-derived repository source/script/binary；`GH_TOKEN=${{ github.token }}` MUST只绑定当前repository的Actions、environment、rulesets、Git ref/Compare及实际GitHub Release API shell steps，不得传入artifact download、本地sealed bundle/remote asset identity校验或其他非API steps。runtime MUST以`includes_parents=false`列出本repository rulesets，按精确name唯一定位`protect-master`与`protect-stable-tags`并GET by ID，以显式canonical projection比较可见的source/source_type/target/enforcement/conditions和rules：无序数组排序，rule type集合精确相等；缺失与disabled/empty `dismissal_restriction`、缺失与空`required_reviewers`、缺失与`false`的`do_not_enforce_on_create`、tag `update`缺失参数与`update_allows_fetch_and_merge=false`分别视为等价安全默认值，其他documented enforcement字段必须逐项相等。可选字段enabled/非空、tag update参数为`true`、额外rule type或未知security-affecting parameter MUST fail-closed；不得接受继承ruleset、只检查active/target或模糊名称。对无ruleset写权限调用者可能隐藏的`bypass_actors`不得假定为空，也不得因此引入admin token，该事实由tag前admin API/UI审计证据承担。Compare step MUST在至多三次循环内取得前后相同的master ref SHA，并校验Compare `base_commit`/`merge_base_commit` SHA、ahead/identical status及`behind_by=0`；Compare schema没有`head_commit`字段，不得读取该字段或只信status。公开后smoke MUST使用匿名HTTPS asset URL，不得调用需要认证的GitHub CLI/API。

release workflow 的每个第三方 Action MUST 固定到经官方 release tag 映射核验的完整 40 位 commit SHA，并在邻近注释可读 tag；JavaScript Action MUST 使用 GitHub-hosted runner 当前受支持的 runtime，不得依赖 compatibility fallback或 `ACTIONS_ALLOW_USE_UNSECURE_NODE_VERSION`。每次 checkout MUST 设置 `persist-credentials: false`。workflow MUST NOT 使用第三方 release Action、`pull_request_target`、`continue-on-error`、clobber/overwrite、用户 PAT 或长期 credential；未知/不合法 ref、版本不一致、事后补跑或缺失的Actions evidence、environment缺失/空保护/未实际review、ruleset漂移、artifact identity、checksum、immutability或API验证错误均 MUST fail-closed。repository immutable releases setting、受保护environment与branch/tag rulesets只能由独立、明确授权的admin操作配置，workflow自身不得获得或使用admin credential。GitHub immutable releases自动生成的platform release attestation不允许workflow扩大`id-token`或`attestations`权限。现有 `.github/workflows/ci.yml` 与 `security-audit.yml` 的只读权限、触发与 job 契约 MUST 保持不变。

#### Scenario: PR 与 dry-run 没有 release 写权限
- **WHEN** release workflow 由 `pull_request` 或 `workflow_dispatch` 触发
- **THEN** 所有实际运行jobs的token权限最多为`contents: read`，publish job不运行、受保护environment不部署，任何step都不能创建tag、Release或asset

#### Scenario: 合法 tag 只给 publish job 最小发布权限
- **WHEN** 严格stable SemVer tag的版本与artifacts已验证且publish job经受保护environment批准后开始
- **THEN** 仅该job获得`contents: write`与`actions: read`，publish job不checkout，`GH_TOKEN`只绑定当前repository的Actions/environment/rulesets/Git ref/Compare及draft/create/upload/API verify/publish shell steps；build与公开后smoke jobs仍为只读，后者通过匿名HTTPS下载且没有token环境变量

#### Scenario: Actions read 不能替代或扩大为 Actions write
- **WHEN** publish job查询同一revision的CI、Security与release dry-run
- **THEN** 查询必须限定当前repository及预期workflow/event/head SHA，枚举`run_attempt`并读取对应attempt-specific job集合，排除不早于tag run创建时间完成的jobs，记录精确`run_id/run_attempt`；token没有`actions: write`且不得cancel、rerun、delete或修改任何workflow run

#### Scenario: Environment 保护不能由 admin bypass 替代
- **WHEN** 合法tag的publish job等待`release` environment
- **THEN** review history必须含`state=approved`、required reviewer及当前environment精确ID/name且没有同environment非approved记录，environment仍包含精确reviewer/self-review/当次tag policy且admin bypass已关闭；删除后隐式创建的空environment、其他environment的批准、rejected history或强制bypass均不得获得Release写入

#### Scenario: 发布 workflow 的 Action 全部不可变且 runtime 受支持
- **WHEN** 静态审查 `release.yml` 的每个 `uses:`
- **THEN** 每个引用均为官方 tag 对应的完整 commit SHA并有邻近 tag 注释，JavaScript runtime 受支持，不存在 floating ref或不安全 runtime override

#### Scenario: Checkout credential 不进入构建环境
- **WHEN** release workflow 在任一事件和平台执行 checkout
- **THEN** 只有官方checkout step可通过默认input使用内建只读token，`persist-credentials: false`生效，后续Cargo、package、checksum与smoke steps的environment及repository config中均没有checkout token

#### Scenario: Publish job 不把 token 交给 checkout、仓库代码或bundle校验
- **WHEN** 合法tag进入具有`contents: write`与`actions: read`的publish job
- **THEN** 该job不执行checkout、Cargo、仓库脚本或release binary，且`GH_TOKEN`只存在于当前repository的Actions/environment/rulesets/Git ref/Compare/Release API shell steps，不存在于artifact download、本地checksum、sealed-to-remote identity或公开下载验证

#### Scenario: 公开后 smoke 不使用认证
- **WHEN** Windows/Linux public smoke jobs验证已公开Release
- **THEN** jobs仅通过匿名HTTPS asset URL下载，不设置`GH_TOKEN`/`GITHUB_TOKEN`环境变量且不调用GitHub CLI/API

#### Scenario: 非法发布输入 fail-closed
- **WHEN** ref/event/version不合法、Actions evidence缺失/不一致/晚于tag边界、environment或rulesets异常、artifact identity/checksum/immutability/API验证异常、已有同tag Release，或任一publish命令失败
- **THEN** workflow以失败状态结束且不得公开部分Release、覆盖既有asset或扩大权限重试

#### Scenario: 现有 CI 与 Security 权限不被发布能力扩张
- **WHEN** 对比本 change 前后的 `ci.yml` 与 `security-audit.yml`
- **THEN** 二者触发条件、job/check名称、`contents: read`、checkout credential、测试和RustSec语义保持不变
