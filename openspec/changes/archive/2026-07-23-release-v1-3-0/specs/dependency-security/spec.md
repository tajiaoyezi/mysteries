## MODIFIED Requirements

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
