## RENAMED Requirements

- FROM: `### Requirement: v1.2.0 只有在公开产物复核后才可归档`
- TO: `### Requirement: stable release 只有在成功验收或失败终止证据完整后才可归档`

## MODIFIED Requirements

### Requirement: Release workflow 严格区分验证与发布事件

仓库 SHALL 提供唯一独立 `.github/workflows/release.yml`。release-sensitive `pull_request` 与 `workflow_dispatch` MUST 只运行 version/package validation，不得创建或修改 tag、GitHub Release 或远端 asset；只有 `push` 一个通过 canonical stable SemVer regex `^v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$` 校验的 annotated tag，且`github.run_attempt == 1`时才可进入 publish path。workflow MUST NOT 使用 `pull_request_target`，publish job MUST 以事件、完整tag ref、metadata与首attempt四重条件 fail-closed并绑定预先配置的受保护`release` environment；任何tag workflow rerun的publish job MUST在environment gate与写token前skip。该environment MUST要求reviewer=`tajiaoyezi`、`prevent_self_review=false`、`custom_branch_policies=true`且唯一policy由admin以`name=v<version>,type=tag`创建，并禁止admin bypass。未知事件、branch push、带前导零或 prerelease/build metadata 的 tag、lightweight tag、手动 dispatch及tag rerun均不得发布。后续 stable version MUST 复用同一 workflow 与事件边界，并经新的admin授权把environment policy更新为自己的精确canonical tag，不得用宽泛fnmatch或为单个版本复制第二条发布入口。

#### Scenario: Release-sensitive PR 只验证 package
- **WHEN** pull request 修改 release workflow、根 manifest/lockfile、release notes 或交付文档
- **THEN** Windows/Linux package validation MUST checkout并验证该PR的`refs/pull/<n>/merge` synthetic merge；所有revision markers必须等于现场读取的该merge ref SHA，Actions API的run `event`必须为`pull_request`且run `head_sha`与已知PR number对应的持久PR endpoint `.head.sha`必须等于目标PR head SHA；run对象的`pull_requests[]`只作可选观察值、为空不得阻断，但publish job不运行，仓库中没有新 tag、draft/public Release 或 Release asset

#### Scenario: 手动 dry-run 永不发布
- **WHEN** 维护者通过 `workflow_dispatch` 在任一 ref 启动 release workflow
- **THEN** workflow 只执行版本、双平台 package与sealed bundle validation，任何 publish或公开下载 verify step均被事件条件排除

#### Scenario: 非稳定 SemVer tag 被拒绝
- **WHEN** workflow 收到 branch push、`v1.3`、`1.3.0`、`v01.3.0`、`v1.03.0`、`v1.3.0-rc.1`、`v1.3.0+build.1` 或其他不匹配 canonical stable tag grammar 的 ref
- **THEN** publish path MUST 不创建或修改 GitHub Release，并以 skip 或显式失败留下可审计结果

#### Scenario: Lightweight tag 被拒绝
- **WHEN** `refs/tags/v<version>` 直接指向 release commit 而不是 annotated tag object
- **THEN** metadata validation MUST 在 package/publish 前失败，即使该 commit、版本与 `origin/master` 完全一致

### Requirement: Tag、源码版本、release notes 与 binary 版本一致

每个可发布 stable version SHALL 从唯一根 package version解析为`version`，对应 tag MUST 精确为`v<version>`、是annotated tag object并指向一个获批release candidate SHA。该candidate MUST 是implementation PR的merge SHA，在候选冻结时等于精确`origin/master` tip，并有同SHA的Windows CI、Ubuntu CI与Security audit全部成功。`workflow_dispatch` release dry-run MUST只在`origin/master`仍等于candidate时以`ref=master`触发，创建出的run `head_sha` MUST精确等于candidate，且metadata、Windows/Linux package与aggregate全部成功而publish不运行；不得假设dispatch API接受raw commit SHA。candidate冻结后，`master`通过受保护PR正常前进 MAY 被允许，但candidate MUST仍可从`master`到达，tag绝不能移动追随新tip。

除人工tag前门禁外，publish job MUST在首次Release API写入前按当前`GITHUB_RUN_ID`读取tag run的server-side `created_at`，再以只读Actions API按workflow file/ID、`event`、`head_sha`与`head_branch`定位同一candidate的master CI、Security与release dry-run，并按每个run的`run_attempt`枚举attempt-specific jobs endpoint。run-level `status`/`conclusion`只代表最新attempt，MUST NOT用于过滤候选；成功、失败与skipped集合必须只从对应attempt-specific jobs判定。精确job contract MUST为：CI仅有`fmt · clippy · test · build (ubuntu-latest)`与`fmt · clippy · test · build (windows-latest)`两个success，Security仅有`RustSec dependency audit`一个success，release dry-run含`Validate release metadata`、`Package release (linux-x86_64)`、`Package release (windows-x86_64)`、`Assemble release bundle`四个success及`Publish GitHub Release`、`Verify published release (windows-x86_64)`、`Verify published release (linux-x86_64)`三个skipped；任何预期job缺失、重复、额外或结论不同均不合格。

每类证据 MAY有多个合格run/attempt；只有其全部预期jobs的server-side `completed_at`严格早于tag run `created_at`的attempt才可入选，并 MUST按`(max(job.completed_at), run_id, run_attempt)`降序选择首项、记录精确`run_id/run_attempt`，且所选attempt内部每个预期job名称与结论必须唯一。tag push后才完成的首次run或rerun attempt MUST NOT成为发布证据，较晚rerun也不得抹掉较早有效attempt。只按显示名称、最新run、branch tip或其他SHA匹配 MUST NOT被接受。release workflow MUST断言tag、根`Cargo.toml` package version、`Cargo.lock`中根package version、`CHANGELOG.md`唯一对应带日期heading，以及各平台release binary的`mysteries --version`输出都等于该版本；tag-triggered metadata还 MUST从annotated tag object读取tagger timestamp并转换为UTC日期，要求与该heading日期相等。合格attempt缺失、所选attempt内部job歧义、字段、日期或结论不一致 MUST在创建draft Release前失败，但多个独立合格run/attempt本身 MUST NOT造成永久歧义。

repository MUST 在implementation merge/tag前经独立admin授权启用并验证immutable releases、受保护`release` environment及两个精确repository ruleset。`protect-master` MUST为`source_type=Repository`、`source=$GITHUB_REPOSITORY`、`target=branch`、`enforcement=active`、`conditions.ref_name.include=["refs/heads/master"]`、`exclude=[]`，规范化rules MUST且只能包含：`pull_request`（`allowed_merge_methods=["merge","squash","rebase"]`、`required_approving_review_count=0`、`dismiss_stale_reviews_on_push=false`、`require_code_owner_review=false`、`require_last_push_approval=false`、`required_review_thread_resolution=false`，创建时不配置dismissal restriction或beta required reviewers）、`required_status_checks`（context精确为上述Windows/Ubuntu/RustSec三个名称且每项`integration_id=15368`、`strict_required_status_checks_policy=true`、`do_not_enforce_on_create=false`）、`deletion`、`non_fast_forward`。`protect-stable-tags` MUST为同一source、`target=tag`、active、include仅`["refs/tags/v*"]`、exclude空，规范化rules MUST且只能包含无参数语义的`update`与`deletion`；两组ruleset MUST无常驻bypass。`release` environment MUST有reviewer=`tajiaoyezi`、`prevent_self_review=false`、`custom_branch_policies=true`且唯一policy由admin以`name=v<version>,type=tag`创建，并关闭admin bypass。workflow不得获得`administration`权限。

每个执行checkout的release job MUST输出恰好一个`RELEASE_REVISION=<40hex>` marker，且同一次run的markers MUST全部等于checkout后的`git rev-parse HEAD`；tag run中还 MUST等于tag peeled commit、run `head_sha`与candidate SHA。metadata provenance因已有checkout，MUST fetch受保护`origin/master`并用`git merge-base --is-ancestor "$REVISION" origin/master`验证candidate可达。publish job MUST保持不checkout、不执行任何checkout-derived repository source/script/binary；publish-lock仅在该API step设置`GH_TOKEN`并执行最多三次稳定读取循环：GET当前repository `heads/master` ref得到`MASTER_BEFORE`，以精确`REVISION...MASTER_BEFORE`调用Compare API并要求`base_commit.sha=REVISION`、`merge_base_commit.sha=REVISION`、`status`为`ahead`或`identical`且`behind_by=0`，再GET同一ref得到`MASTER_AFTER`；只有前后SHA相等才可通过，变化时重试整个循环，三次均不稳定则fail-closed。Compare响应没有`head_commit`字段，workflow MUST NOT读取或假造该字段；head身份由精确请求参数与两次一致的ref响应证明。两处都 MUST NOT保留candidate等于master tip的断言，publish也 MUST NOT依赖不存在的本地Git graph或仅凭Compare status判定。stable tag MUST只在implementation PR merge后由已获该精确SHA明确授权的Git写操作创建，且用于发布的每个证据job必须已经完成，MUST NOT在PR head或synthetic merge-ref上创建。创建本地annotated tag后、push前 MUST从tag object读取tagger timestamp并转换为UTC日期，要求精确等于本version Changelog heading；不一致时 MUST不push并先走日期修正PR、完整门禁与新tag授权。

publish job MUST以repository与完整tag ref为concurrency key串行执行、`cancel-in-progress: false`。tag run到达`release` environment等待点后，维护者 MUST向用户展示精确run/candidate、pre-tag evidence与admin settings重读结果，并为本次deployment另取明确批准；tag创建/push授权 MUST NOT被视为environment approval，也不得以admin bypass代替。取得environment gate后、首次GitHub Release API写操作前，publish job MUST先读取当前`release` environment ID/name，再通过`/actions/runs/{run_id}/approvals`证明存在`state=approved`、`user.login=tajiaoyezi`且`environments`包含该精确ID/name的review；同environment存在任何非approved review、仅用户名命中、其他environment批准、空history或admin bypass MUST fail-closed。它还必须通过只读environment API重验environment仍存在、required reviewer、`prevent_self_review=false`及`custom_branch_policies=true`，分页读取deployment-branch-policies并要求总数为1、唯一name精确等于`v<version>`。只读policy响应schema未承诺回显branch/tag `type`，runtime MUST NOT依赖未文档化字段；当前tag job已通过environment gate证明该pattern适用，`type=tag`由tag前admin API/UI证据证明。

runtime MUST以`includes_parents=false`列出本repository rulesets，按精确name各唯一定位`protect-master`与`protect-stable-tags`，再GET其ID并构造显式canonical projection：无序数组排序，比较`source_type`、`source`、`target`、`enforcement`、完整`conditions.ref_name.include/exclude`，且rule type集合必须与上文contract精确相等。`pull_request.dismissal_restriction` MAY缺失或精确为`{enabled:false,allowed_actors:[]}`，`required_reviewers` MAY缺失或为空数组，`required_status_checks.do_not_enforce_on_create`缺失时 MUST按API默认`false`归一化；tag target的`update.parameters.update_allows_fetch_and_merge` MAY缺失或为`false`，不得为`true`。其他documented enforcement字段 MUST逐项精确相等。任一可选默认字段enabled或非空、未知security-affecting parameter、缺失/重复/额外ruleset或rule、错误source/include/exclude/required check `integration_id`均为漂移；rule外response metadata MAY忽略，继承ruleset不得代替repository-owned contract。rulesets API MAY对无写权限调用者隐藏`bypass_actors`，runtime MUST NOT把字段缺失解释为空bypass，也不得为读取该字段引入admin credential；ruleset无常驻bypass与environment禁止admin bypass MUST由tag前admin API/UI审计证据证明。随后以不带`--refs`的匿名`git ls-remote --tags`读取全部tag refs并在本地按完整ref名精确过滤tag object ref与`^{}` peeled ref。tag refs必须精确各一条且SHA不同，peeled SHA必须等于run/candidate revision；任何environment缺失/空保护、缺少实际review、deployment policy缺失/额外/宽泛、tag漂移、ruleset contract漂移或Compare ancestry不成立 MUST在创建draft前fail-closed。实现 MUST NOT依赖把`refs/tags/<tag>^{}`作为单独remote pattern查询，也 MUST NOT因为`master`tip在candidate冻结后经受保护PR正常前进而误判失败。

#### Scenario: 精确 master merge 通过远端门禁后冻结 candidate
- **WHEN** v1.3.0 implementation PR已按active master ruleset合入，候选merge SHA在冻结时是精确`origin/master` tip，且该SHA唯一对应的master Windows/Ubuntu CI、Security audit与release dry-run均成功
- **THEN** repository immutable releases、受保护`release` environment、master branch ruleset与`v*` tag ruleset已另行获批并生效；维护者在看到精确candidate并明确批准后才可创建annotated `v1.3.0`，本地tagger UTC日期与Changelog一致且peeled commit等于candidate SHA后才可push

#### Scenario: 合法 tag 也不能替代既有门禁
- **WHEN** candidate上存在canonical annotated tag，但该revision的master CI、Security或release dry-run任一缺失、失败、仍在运行或job集合异常
- **THEN** publish job MUST 在首次Release API写入前通过Actions evidence preflight失败，即使metadata、package与aggregate均成功也不得创建draft

#### Scenario: Tag 后补跑的证据无效
- **WHEN** 同SHA的CI、Security或dry-run attempt只在当前tag run的server-side `created_at`之后完成或rerun成功
- **THEN** publish job MUST排除这些attempt并在无更早合格attempt时fail-closed；若同一run存在更早有效attempt则仍可按`run_id/run_attempt`引用它，不得让事后rerun覆盖历史或绕过tag前门禁

#### Scenario: 最新 rerun 不覆盖较早有效 attempt
- **WHEN** 一个run的较早attempt在tag run创建前完整成功，但其最新attempt失败或仍在运行，导致run-level `status`/`conclusion`不再表示早期结果
- **THEN** publish job MUST忽略run-level结果并从attempt-specific jobs独立验证、选择较早attempt；不得误拒绝candidate，也不得把较晚attempt当作早期证据

#### Scenario: 多个合法 dry-run attempt 可确定性选一个
- **WHEN** 同一candidate存在多个在tag run创建前完成且job集合完整成功的workflow_dispatch run/attempt
- **THEN** publish job MUST按`(max(job.completed_at), run_id, run_attempt)`降序选中首项并记录精确`run_id/run_attempt`；不得仅因存在多个独立合格run/attempt而永久阻断candidate

#### Scenario: PR head 或未验证 commit 不得成为 release tag
- **WHEN** 候选 tag 指向未合入 `master` 的 PR head、synthetic merge-ref，或没有精确成功 CI/Security/dry-run证据的 commit
- **THEN** release 操作 MUST 停止且不得 push tag或创建 GitHub Release

#### Scenario: Master 正常前进不使已验证 candidate 失效
- **WHEN** candidate冻结或tag push后，`master`通过受保护PR前进到新tip，但tag peeled commit仍精确等于获批candidate、candidate仍是master ancestor且全部Actions/ruleset证据不变
- **THEN** release workflow MUST继续验证并发布该candidate，MUST NOT要求移动tag追随新master tip，也不得仅因tip不相等而失败

#### Scenario: Candidate 可达性或保护漂移阻断发布
- **WHEN** metadata checkout发现candidate不再是`origin/master` ancestor，或publish锁内三次均无法取得前后相同的master ref、Compare的base/merge-base/status/behind_by任一不满足ancestor contract，或实现读取不存在的`head_commit`字段，或发现environment缺失/保护漂移/未实际review、任一精确ruleset contract失效、tag peeled SHA变化
- **THEN** workflow MUST在任何Release API写操作前失败，远端不得出现draft、asset或public Release

#### Scenario: 同一 tag 的发布尝试串行且不可互相取消
- **WHEN** 同一repository与tag存在两个publish job尝试
- **THEN** concurrency MUST 保证远端变更串行且不取消已开始的publish；后续尝试在锁内预检既有Release或漂移后fail-closed，不得竞争创建或覆盖draft

#### Scenario: 任一版本事实不一致即失败
- **WHEN** tag、`Cargo.toml`、`Cargo.lock`、Changelog heading 或任一 binary `--version` 中至少一个不是同一稳定版本
- **THEN** workflow MUST 在创建 draft Release 前失败，并明确指出不一致的事实源

#### Scenario: Release jobs 测试同一 tag revision
- **WHEN** tag run 的任一job执行checkout
- **THEN** 该job日志恰有一个 `RELEASE_REVISION` marker，所有marker与run `head_sha`、tag peeled commit及release candidate SHA完全一致；不执行checkout的publish/public verify job不得伪造marker

### Requirement: Windows 与 Linux release package 可复现且可独立验证

release workflow SHALL 在 GitHub-hosted `windows-2022` 与 `ubuntu-22.04` 上使用固定 Rust `1.96.1`，分别为 `x86_64-pc-windows-msvc` 与 `x86_64-unknown-linux-gnu` 执行 `cargo +1.96.1 build --release --locked --target <target>`；不得依赖浮动 `stable` 或 `*-latest` 作为release compiler/runner事实源。Linux GNU binary的支持baseline SHALL 为x86_64 glibc 2.35-compatible environment，workflow MUST 从 ELF version requirements计算并断言不存在高于 `GLIBC_2.35` 的required symbol。每个平台 MUST 在原生 runner 上对刚构建的 binary执行 `--version` 与 `--help`，要求exit 0、版本一致且不读取provider credential、不访问网络。

对任一待发布`version`，Windows ZIP MUST 命名为`mysteries-v<version>-x86_64-pc-windows-msvc.zip`且只含`mysteries.exe`、`LICENSE`、`README.md`；Linux tar.gz MUST 命名为`mysteries-v<version>-x86_64-unknown-linux-gnu.tar.gz`且只含`mysteries`、`LICENSE`、`README.md`。archive内不得包含repository credential、config、session、target中间产物或绝对路径。生成的binary、archive与checksum MUST只作为workflow/GitHub Release asset交付，不得提交到Git。

#### Scenario: Windows package 原生 smoke
- **WHEN** `windows-2022` release job使用固定Rust从已锁定`version`源码构建并展开`mysteries-v<version>-x86_64-pc-windows-msvc.zip`
- **THEN** archive只含约定文件，`mysteries.exe --version`精确报告该`version`，`--help`成功，workflow artifact名称不与Linux冲突

#### Scenario: Linux package 原生 smoke
- **WHEN** `ubuntu-22.04` release job使用固定Rust从已锁定`version`源码构建并展开`mysteries-v<version>-x86_64-unknown-linux-gnu.tar.gz`
- **THEN** archive只含约定文件，`mysteries --version`精确报告该`version`，`--help`成功、executable bit可用，ELF target为x86_64 GNU且required GLIBC symbol version不高于2.35

#### Scenario: 发布产物不回写仓库
- **WHEN** 审查 release implementation diff 与最终 tag tree
- **THEN** 不存在新增当前或未来 stable version的executable、archive或checksum binary blob；既有v1.1.0 executable仅保留为历史验证产物

### Requirement: Checksums 与 draft-to-public 发布 fail-closed

只读 `Assemble release bundle` job SHALL 从双平台 jobs 下载恰好两个由当前`version`派生的预期 archive，拒绝缺失、重复、额外 archive、symlink 与不匹配 version/target triple 的名称，并生成 UTF-8 `SHA256SUMS`，其中每个 archive 恰有一条 SHA-256 记录；它 MUST 本地验证 checksums，并上传只含两个 archive、`SHA256SUMS`与release notes的sealed bundle。publish job SHALL 只下载该sealed bundle并在无token环境的本地步骤重新验证其精确文件集与checksums；取得受保护environment批准、tag级concurrency锁并完成Actions/rulesets/candidate/tag/provenance重验后，才以`v<version>` tag创建非 prerelease 的 draft GitHub Release并上传两个 archive与`SHA256SUMS`。

随后 publish job MUST 从paginated Release列表按tag筛选恰好一个Release ID，通过`/releases/{release_id}`重新读取draft的`tag_name`、`target_commitish`、draft/prerelease状态、`.body`及asset名称/数量/size，并按每个asset ID从`/releases/assets/{asset_id}`下载。remote `SHA256SUMS`与两个remote archives MUST 分别逐字节等于本run sealed bundle中的对应文件，再使用本地sealed manifest验证remote archives；draft API JSON解码后的`.body` MUST逐字节等于sealed `release-notes.md`，不得做换行、空白或编码规范化。只证明remote manifest与remote archives彼此自洽，或只人工观察Release正文语义相似，MUST NOT 被接受为identity证据。API若提供asset digest则同时核对本地hash。`tag_name` MUST 精确等于`v<version>`；`target_commitish` MUST 非空并作为观察值记录，但不得被当作不可变commit SHA或revision权威，因为GitHub对既有tag可返回branch名。发布revision的唯一权威 MUST 是该`tag_name`对应的远端annotated tag peeled commit，并与run/release candidate SHA精确一致。draft阶段MUST NOT依赖仅能解析public Release的`/releases/tags/{tag}`。公开PATCH前 MUST 再次匿名重验remote tag object/peeled ref仍等于run revision。全部成立后必须按同一Release ID转为public/latest；公开成功后，Release API的`immutable` MUST 为`true`且public API JSON解码后的`.body`仍逐字节等于sealed notes，再通过tag/latest endpoints复核。repository immutable releases setting、禁止admin bypass且有实际required review的受保护`release` environment及branch/tag rulesets MUST 在implementation merge/tag之前由已获独立授权的admin操作启用并验证，workflow不得通过PAT、长期credential或扩大`administration`权限自行配置；immutable setting需要admin read，environment reviewer MUST在批准publish前复核该事实，workflow以公开后的`immutable=true`作为平台最终证据。已存在同 tag 的 draft/public Release 或同名 asset 时 MUST fail-closed，不得覆盖；draft创建前失败不得产生Release。tag一旦push，tag workflow任一步失败或取消均使该version立即视为已消耗，不区分零diff/transient，也不取决于candidate、immutable、environment或ruleset是否仍稳定；必须保留失败run、tag及残留draft，不得删除、移动、重建、公开或rerun复用，后续只能走新的patch release change。`master`通过受保护PR正常前进且candidate仍为ancestor不属于attempt 1执行期间的candidate漂移，但失败后仍不得复用该tag。公开后的Windows/Linux verify jobs MUST 不设置`GH_TOKEN`/`GITHUB_TOKEN`环境变量、不调用GitHub CLI/API，而从公开的匿名HTTPS Release asset URL重新下载archive与`SHA256SUMS`，从而证明未登录用户可访问。

#### Scenario: 完整 asset set 与 sealed notes 才可公开
- **WHEN** 当前`version`的两个平台archive、`SHA256SUMS`、draft metadata、release body、重新下载的asset与checksum全部匹配
- **THEN** remote三文件与draft API解码后的`.body`分别逐字节等于本run sealed bundle中的对应文件且远端tag再次等于run revision，workflow才把对应`v<version>` draft Release转为public/latest；公开Release仅含这三个assets、public `.body`仍等于sealed notes并报告`immutable=true`

#### Scenario: 缺失或额外 asset 及正文漂移阻断公开
- **WHEN** 聚合目录或 draft Release 缺少任一 archive/checksum，出现重复/额外 archive、symlink、错误文件名、空文件、checksum不匹配，或draft/public `.body`与sealed `release-notes.md`存在任一字节差异
- **THEN** workflow MUST 失败；若错误发生在draft创建前则不得产生Release，若draft已创建则保持非公开且不得以部分assets发布

#### Scenario: 既有 tag Release 不被覆盖
- **WHEN** 当前`v<version>`已存在draft/public Release或任一预期asset名称已占用
- **THEN** workflow MUST 失败并要求人工审查，MUST NOT 使用 clobber、覆盖上传或删除既有远端对象

#### Scenario: Tag push 后任一失败都不复用版本
- **WHEN** tag已push后tag workflow任一步失败、取消或尝试以`run_attempt > 1` rerun，无论是否已有draft、是否为零diff/transient以及candidate/immutable/environment/rulesets是否仍稳定
- **THEN** 原version、tag、失败run与残留draft MUST保留为失败证据且不得删除、移动、重建、公开或rerun复用；修复必须使用新的patch version及独立change完成

#### Scenario: 公开 asset 可匿名下载
- **WHEN** draft已转public且Windows/Linux verify jobs开始下载对应archive与`SHA256SUMS`
- **THEN** jobs MUST 通过公开HTTPS URL在没有`GH_TOKEN`/`GITHUB_TOKEN`环境变量及GitHub CLI认证的情况下下载并完成checksum与smoke验证

### Requirement: Release notes 与安装文档如实描述版本历史和兼容性

每次 stable release时，`CHANGELOG.md` SHALL 保留新的空`Unreleased` heading，并把本次内容固化为唯一带annotated tag创建UTC日期的`[<version>]` section；创建本地annotated tag后、push前 MUST 读取实际tagger timestamp并转换为UTC日期再次与heading比较，tag-triggered metadata MUST独立重复相同检查并在不一致时阻断publish。任一tag前检查、本地tag object或workflow机器检查的日期与heading不一致时，维护者 MUST 不push或停止发布；尚未push则移除未发布本地tag并先经新的implementation PR修正日期、重跑该精确revision的全部门禁及重新取得tag授权，已经push则按已消耗version处理，不得给日期不一致的旧revision发布Release。既有版本与链接不得重写。v1.2.0 SHALL 继续标注为首个自动化、可复现的GitHub Release，`1.0.0`/`1.1.0`继续标注为未创建Git tag/GitHub Release的开发里程碑。

根README MUST提供当前stable Windows/Linux Release asset下载、`SHA256SUMS`校验、解压、`--version`验证与源码构建路径。`deliverables/README.md` MUST把该目录与正式分发隔离，标明其中binary仅为历史验证产物，指向GitHub Releases及根README安装说明，并保留源码构建入口；它 MUST NOT重复维护完整下载/checksum/解压命令或把历史binary描述为当前安装源。candidate README MUST把本次已冻结能力标记为待发布version而非`Unreleased`，使正式ZIP/tar.gz内的README准确描述所含功能；安装命令 MUST从公开`releases/latest`解析真实stable tag并由该tag派生versioned asset名称，或在新Release公开前继续固定指向上一个已公开stable，implementation merge MUST NOT提前引用尚不存在的`v<version>` URL。新Release公开且匿名下载验证完成后，维护者 MAY通过post-release docs/archive PR补齐新version直接历史/Release链接；该docs commit MUST NOT移动或重建已发布tag。

若stable tag push后的attempt 1失败，Changelog与版本链接 MUST如实区分“源码/tag已固化”与“public Release已交付”：保留失败version的annotated tag与带日期历史，但明确其Release未公开、版本已消耗且latest仍指向前一个成功stable；不得把非公开draft链接或残留assets描述为安装源。后续patch SHALL承接同一能力集并明确自己才是首次公开交付。v1.3.0 release notes MUST继续如实说明Agent execution scope与单层只读`delegate_task`的已交付边界及资源限制，但 MUST同时记录annotated tag、失败run与非公开draft被保留，v1.3.0未成为public/latest Release且不能复用。

#### Scenario: v1.3.0 失败历史保持真实
- **WHEN** annotated `v1.3.0` tag的attempt 1在创建非公开draft后失败且该version按contract消耗
- **THEN** Changelog保留带日期的v1.3.0源码/tag历史并明确其未公开、draft仅作失败证据、latest仍为v1.2.0；README安装命令不得把v1.3.0 draft assets当作正式分发

#### Scenario: Implementation merge 不提前产生 404
- **WHEN** v1.3.0 implementation PR已合入但tag/Release尚未公开
- **THEN** README可如实标明源码/候选版本为v1.3.0，但安装命令仍解析到真实公开的v1.2.0 assets或其他实际latest stable，不引用不存在的public v1.3.0 URL

#### Scenario: 既有发布历史保持真实
- **WHEN** 阅读版本链接与历史release notes
- **THEN** 1.0/1.1仍是无tag/Release的开发里程碑，v1.2.0仍是首个自动化public Release，v1.3.0只链接真实tag并明确非公开失败状态且不改写旧版本内容

#### Scenario: 安装说明只引用正式 assets
- **WHEN** 用户按README下载当前stable预编译binary
- **THEN** 下载路径指向该版本GitHub Release的Windows/Linux versioned archive与`SHA256SUMS`，不把`deliverables/`中的历史executable当成当前安装源

### Requirement: stable release 只有在成功验收或失败终止证据完整后才可归档

成功stable release的implementation PR、精确candidate SHA之master CI/Security、同SHA的release dry-run、所选`run_id/run_attempt`、精确repository rulesets、受保护environment审批、publish Actions/ruleset evidence preflight、`v<version>` tag workflow、sealed-to-remote asset/body identity、public GitHub Release的`tag_name`/`immutable=true`与远端peeled tag、下载后验证及post-release README状态 SHALL形成同一change的完成证据；`target_commitish`只作为非空观察值，不参与SHA一致性判定。PR长期provenance MUST使用run `head_sha`与已知PR endpoint `.head.sha`，不得依赖可能在merge后为空的run `pull_requests[]`。公开后 MUST在Windows与Linux分别从GitHub Release重新下载对应archive及`SHA256SUMS`，验证checksum、文件集、`--version`与`--help`；Windows Terminal还 MUST真机启动并正常退出TUI，退出后PowerShell输入立即正常。成功链任一步未完成或权威证据指向不同SHA/version时，tasks不得标记为成功完成，change不得以成功发布名义archive。

若tag push后的attempt 1失败或取消且contract禁止复用该version，change MAY以`terminated-by-failure`归档，但必须保留失败run、annotated tag、approval、残留draft/assets原状，勾选失败分流并保持未发生的publish/public/smoke tasks未勾；还必须建立独立patch change、记录失败step与authoritative identity/checksum证据，并让archive决策记录明确“归档不代表Release成功”。该例外不得删除、移动、重建、公开或rerun失败对象，也不得满足任何public release成功Scenario。

#### Scenario: 全链证据一致后允许 archive
- **WHEN** 某stable version的implementation PR merge、master CI/Security、dry-run、两个精确rulesets、protected environment、publish Actions/ruleset evidence、tag peeled commit、release workflow、sealed/remote asset与body identity、GitHub Release `immutable=true`及`tag_name`所解析的peeled tag与两个下载后binary version全部指向同一candidate，且post-release README验证完成
- **THEN** change可在记录`run_id/run_attempt`、job/ruleset/environment/release/asset/body identity/checksum/immutable/真机证据并经用户审阅archive决策记录后进入archive

#### Scenario: Tag发布失败可按证据终止归档
- **WHEN** tag push后的attempt 1失败，失败version、run、tag与draft/assets均保留，成功发布tasks保持未勾，失败分流已勾且后续patch change已建立
- **THEN** change可在记录失败step、candidate/tag/draft/asset identity与禁止复用决策并经用户审阅后以`terminated-by-failure`带warning归档；该状态MUST NOT声称Release已公开或验收成功

#### Scenario: 公开 Release 不等于自动完成 change
- **WHEN** GitHub Release已公开，但任一下载后checksum/smoke、Windows Terminal真机或证据一致性尚未验证
- **THEN** change MUST保持active，且不得把Release可见性代替最终验收
