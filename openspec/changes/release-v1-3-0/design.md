## Context

`master` 当前为 v1.2.0，v1.3 计划中的 `add-agent-execution-scope` 与 `add-readonly-subagent` 已分别完成自动化、真机验收和归档。`CHANGELOG.md` 的 `[Unreleased]` 已只包含这两组 v1.3 能力，README 也将单层只读委派明确标为 `[Unreleased]`；远端唯一 stable tag / GitHub Release 仍是 v1.2.0。

仓库已有 `.github/workflows/release.yml`：PR 与 `workflow_dispatch` 只做 metadata、Windows/Linux package 和 sealed bundle validation；只有指向精确 `origin/master` tip 的 annotated stable SemVer tag 才进入 draft-to-public publish 与匿名公开下载验证。workflow 的 version、archive basename、bundle 和公开 URL 已从根 package version动态派生，原则上无需为 v1.3.0复制或重写发布系统。

冲突点在主 `release-delivery` spec：实现已版本参数化，但 contract 仍大量硬编码 v1.2.0，并混合了“首次自动化 Release”的一次性历史治理要求。对抗式核对还确认publish job当前只依赖metadata输出，缺少job级event/ref直接复核，也不会查询同一revision的CI/Security/dry-run；draft下载后只用远端`SHA256SUMS`验证远端archives彼此自洽，未证明它们等于本run sealed bundle。GitHub Release API的`target_commitish`在既有v1.2.0 Release中为`master`而非commit SHA，不能作为revision权威。

远端现场事实同样不满足“公开后不可重写”：repository immutable releases当前`enabled=false`，v1.2.0 Release的`immutable=false`，repository rulesets为空且master未启用legacy branch protection。既有成功PR Release run在PR merge后仍保留正确`head_sha`，但run对象的`pull_requests`数组可变为空；持久PR endpoint仍保留head SHA。因此v1.3.0发布前必须让spec、workflow与真实平台门禁对齐，而不能把可消失字段或人工纪律写成机器不变量。

本 change 跨版本事实源、71 份版本敏感 TUI snapshots、PR/master/tag 三类 GitHub Actions 事件、远端 tag/Release 与 archive 证据，真正的 tag 和公开发布只能发生在 implementation PR 合入之后。

## Goals / Non-Goals

**Goals:**

- 把同一精确 source revision 固化为 package、lockfile、Changelog、binary 与 tag 均一致的 v1.3.0。
- 复用既有固定 toolchain、双平台原生 package、checksums、sealed bundle、draft-to-public 和匿名公开下载验证链。
- 将 `release-delivery` 从 v1.2.0 一次性描述泛化为后续 stable release 可复用的规范，同时为 v1.3.0 保留可审计的完成门禁。
- 严格区分 implementation、master dry-run、tag publish 和 archive 四个阶段，禁止在 PR head 或 synthetic merge-ref 上打 tag。
- 让版本升级引起的 snapshot diff 只有 `v1.2.0` → `v1.3.0` 字面量变化，布局、样式、metadata 与其他正文零 churn。
- 让publish job在首次Release API写入前以最小`actions: read`权限机器验证同一revision的master CI、Security与release dry-run，并由受保护`release` environment承接最终平台审批。
- 用无常驻bypass的active `master` branch ruleset保证candidate来自PR与required checks且不能被force rewrite，用同样无常驻bypass的active `v*` tag ruleset保证tag从创建起不能更新/删除；允许`master`在candidate获批后正常前进，消除“检查tip→push tag”之间不可封闭的TOCTOU。
- 让draft remote assets逐字节等于本run sealed bundle，并在正式公开后由GitHub immutable releases锁定assets与关联tag。

**Non-Goals:**

- 不新增或修改 execution scope、`delegate_task`、subagent budget、递归、后台任务、child session、专用 model、Network/Edit/Execute child。
- 不新增 dependency，不修改 Rust runtime/API、Provider、Tool、Permission、Config 或 session wire。
- 不增加 macOS/ARM、installer、package manager、crates.io、代码签名、SBOM、自定义SLSA/provenance workflow、`id-token`或attestation Action；启用GitHub immutable releases后由平台自动生成的release attestation属于setting既有效果。
- 不追求跨时间或不同runner的bit-for-bit reproducible binary/archive；跨PR、dry-run与tag run只要求revision、version、release notes语义、文件集和各run内部checksum验证一致，不要求不同run的archive digest相等。
- 不改变 CI/Security workflow 的 triggers、权限或 job 名称；release workflow 也不因单纯版本升级重构。
- 不回填、移动或重写既有 v1.2.0 tag/Release，不把生成的 v1.3.0 binary/archive/checksum提交进 Git。

## Decisions

### D1 · implementation diff 是版本固化，不是功能开发

根 `Cargo.toml` package version改为 `1.3.0`，由 Cargo 只更新 `Cargo.lock` 中根 `mysteries` entry。不得执行无界 `cargo update`，dependency graph、MSRV 与 release toolchain保持不变。

`CHANGELOG.md` 顶部继续保留一个空 `[Unreleased]`，把当前两条能力固化到 `[1.3.0] - <annotated tag创建的UTC日期>`；v1.2.0及更早历史原样保留。implementation先写计划tag日期，真正创建tag前必须重新核对UTC日期；若已跨日则先用新的implementation PR更新Changelog日期并重跑全部精确门禁，不得给旧revision打tag。

README不能在v1.3.0公开前把当前stable URL硬编码为不存在的tag/asset。implementation PR把单层只读委派的`[Unreleased]`标记固化为v1.3.0，同时把安装命令改为从公开`releases/latest`解析真实tag并由该tag派生versioned asset名称：合入后仍下载v1.2.0，v1.3.0公开后自动解析到v1.3.0。这样正式archives内的README准确标识所含v1.3.0能力，默认分支也不制造404窗口；v1.3.0直接历史/Release链接只在公开验证后的post-release docs/archive PR补齐。`deliverables/README.md`已声明v1.2.0及后续不提交binary，若无需改变事实则保持零diff。

备选“发布前顺便补 aggregate child budget”被弃：它会新增 headless 行为与 TDD链，破坏已完成的 v1.3 scope冻结。该风险继续公开披露并留给下一 feature change。

### D2 · 通用 contract 使用待发布稳定版本，并在唯一 workflow 内闭合发布门禁

`release-delivery` 前五个既有 Requirement 保留原 header；第六个从 v1.2.0 一次性标题重命名为通用 stable release 完成条件。各 Requirement 中 v1.2.0 专用的 tag、asset、版本输出与归档叙述改为 `<version>` 语义：`version`来自唯一根 package，`tag = v<version>`，两个 archive basename也由该 version与固定 target triple组成。v1.2.0仍作为首个自动化 Release 的历史事实；v1.3.0 Scenario 验证当前第二次 stable release走同一 contract。

`.github/workflows/release.yml` 已动态读取 version并生成 archive/bundle名称，package与aggregate构建路径保持不变。publish job必须同时要求`github.event_name == 'push'`、`github.ref`位于`refs/tags/`、metadata publish=true及`github.run_attempt == 1`；任何tag workflow rerun的publish job在取得environment gate和写token前直接skip。canonical stable SemVer与annotated tag仍由metadata shell做完整校验，避免在GitHub expression中复制不等价regex。tag-triggered metadata还必须从annotated tag object读取tagger timestamp、转换为UTC日期并与唯一Changelog version heading日期相等，使人工tag前复核不成为唯一防线。

现有metadata与publish-lock分别各有一处`peeled == master tip`硬断言，但两个job的执行环境不同，不能用同一实现替换。metadata已经checkout完整history，必须fetch受保护`origin/master`并用`git merge-base --is-ancestor "$REVISION" origin/master`证明candidate可达；publish job明确不checkout、也不执行任何checkout-derived repository source/script/binary，在仅该API step注入`GH_TOKEN`并执行最多三次的稳定读取循环：GET当前repository `heads/master` ref得到`MASTER_BEFORE`，以精确`REVISION...MASTER_BEFORE`调用Compare API，要求`base_commit.sha == REVISION`、`merge_base_commit.sha == REVISION`、`status`为`ahead`或`identical`且`behind_by == 0`，再GET同一ref得到`MASTER_AFTER`；只有前后SHA相等才通过，否则重试整个循环，三次均变化则fail-closed。Compare响应没有`head_commit`字段，head身份由精确请求参数与两次一致的ref响应共同证明。任何一处保留tip equality、让publish取得repository worktree并执行其中内容、读取不存在的字段，或只检查Compare `status`而不校验现有SHA字段，都会破坏contract。

publish job继续是唯一`contents: write` job，但额外声明最小`actions: read`，并绑定预先配置的`release` environment。该environment明确要求reviewer=`tajiaoyezi`、`prevent_self_review=false`、仅custom tag policy精确`v1.3.0`可deploy，并在UI关闭admin bypass；单一admin仍可作为自己触发run的required reviewer，但不能用bypass跳过review。取得environment批准和tag级concurrency锁后、首次Release API写入前，publish job先读取当前`release` environment ID/name，再通过`/actions/runs/{run_id}/approvals`证明存在`state=approved`、`user.login=tajiaoyezi`且`environments`包含该精确ID/name的review，并拒绝同environment的任何非approved review记录；仅用户名命中、其他environment的批准或空history都不成立。随后按当前`GITHUB_RUN_ID`读取tag run的server-side `created_at`作为不可回填边界，最后通过Actions API按workflow file/ID、`event`、`head_sha`与`head_branch`查询同一`REVISION`的`ci.yml` master push run、`security-audit.yml` master push run及`release.yml` workflow_dispatch run。run-level `status`/`conclusion`只反映最新attempt，不作为候选过滤条件；Windows/Ubuntu CI、RustSec以及dry-run metadata、Windows/Linux package、assemble success和publish/public verify skipped均由对应attempt-specific jobs证明。

每类证据允许存在多个合法run/attempt；publish按run的`run_attempt`枚举attempt-specific jobs endpoint，只保留其全部预期jobs的server-side `completed_at`均严格早于tag run `created_at`的attempt，再按`(max(job.completed_at), run_id, run_attempt)`降序确定性选择首项并记录精确`run_id/run_attempt`，所选attempt内部job名称/结论必须唯一。精确job contract为：CI必须且仅包含`fmt · clippy · test · build (ubuntu-latest)`与`fmt · clippy · test · build (windows-latest)`两个success；Security必须且仅包含`RustSec dependency audit`一个success；dry-run必须包含`Validate release metadata`、`Package release (linux-x86_64)`、`Package release (windows-x86_64)`、`Assemble release bundle`四个success，以及`Publish GitHub Release`、`Verify published release (windows-x86_64)`、`Verify published release (linux-x86_64)`三个skipped。这样重复dry-run不会永久烧掉candidate，而tag push后才补跑或rerun完成的attempt永远不能被选中；tag后rerun也不会抹掉更早有效attempt。只按显示名称、最新run、branch tip或其他SHA匹配均不成立；合格attempt缺失、所选attempt内部job集合歧义或结论异常必须在任何Release写入前失败。

publish还通过只读environment API确认required reviewer、`prevent_self_review=false`与`custom_branch_policies=true`，再分页读取deployment-branch-policies并要求仅有一个pattern、名称精确为`v1.3.0`；该只读响应schema未承诺回显branch/tag `type`，因此runtime不得依赖未文档化字段，当前tag job已通过environment gate证明pattern适用于该tag，而`type=tag`由tag前admin API/UI配置证据承担。两个repository ruleset使用稳定名称与完整JSON contract：`protect-master`要求`source_type=Repository`、`source=$GITHUB_REPOSITORY`、`target=branch`、`enforcement=active`、`conditions.ref_name.include=["refs/heads/master"]`、`exclude=[]`，且规范化rules精确为`pull_request`、`required_status_checks`、`deletion`、`non_fast_forward`；`protect-stable-tags`除`target=tag`、include为`["refs/tags/v*"]`外，规范化rules精确为无参数语义的`update`与`deletion`。`pull_request`参数固定为允许现有三种merge方法、0 required approvals及其余review booleans=false；`required_status_checks`固定为上述三个master check contexts，每项`integration_id=15368`（GitHub Actions），并要求`strict_required_status_checks_policy=true`、`do_not_enforce_on_create=false`。runtime以`includes_parents=false`列出本repository rulesets，按精确name各定位唯一项、再GET by ID，并以显式canonical projection比较source/target/enforcement/conditions和rules：无序数组先排序，rule type集合必须精确相等；`pull_request.dismissal_restriction`仅允许缺失或精确回显`{enabled:false,allowed_actors:[]}`，`required_reviewers`仅允许缺失或空数组，`required_status_checks.do_not_enforce_on_create`缺失按API默认`false`归一化；tag target的`update.parameters.update_allows_fetch_and_merge`仅允许缺失或`false`，不得为`true`，因为该字段描述branch upstream fetch/merge且GitHub GET tag ruleset通常不回显它。其他documented enforcement字段必须逐项等于批准contract。上述可选默认字段一旦enabled或非空、出现额外rule type或未知security-affecting parameter均fail-closed；rule外的response metadata不参与等价判断。这样既不把GitHub合法的disabled/empty默认回显误判为漂移，也不把继承ruleset或“存在某个active ruleset”当作等价。GitHub对无ruleset写权限调用者可能隐藏`bypass_actors`，因此ruleset无常驻bypass与environment禁止admin bypass均使用tag前admin API/UI审计证据；runtime用review history中的实际approval排除bypass路径，不得把缺失字段误当空bypass，也不得为读取它引入admin credential。`GH_TOKEN`只进入这些只读API preflight、Compare与既有Release API steps，不进入checkout、仓库代码、构建、bundle校验或public verify。

draft assets通过asset ID下载后，必须先逐字节比较本地sealed bundle与remote `SHA256SUMS`及两个archives，再使用本地sealed manifest验证remote archives；不得用可与archives同时被替换的remote manifest作为唯一信任根。draft与public Release API返回的`.body`经JSON UTF-8解码后也必须逐字节等于sealed `release-notes.md`，不做会隐藏编码或换行漂移的规范化。API若返回asset digest则同时核对，但digest字段缺失不得替代逐字节identity。

备选“为 v1.3.0 复制一份 workflow”被弃：会形成两个发布入口、权限面与维护源。备选“只改文档、不改硬编码主 spec”被弃：archive 后 code/spec 将继续冲突。

### D3 · 版本敏感 snapshots 走机械字面量迁移

生产 TUI 通过 `env!("CARGO_PKG_VERSION")`显示版本，当前有 71 份 tracked snapshot包含 `v1.2.0`。版本 bump 后统一由测试生成/核对 `v1.3.0` baseline；审查必须证明每份 snapshot 仅发生版本字面量替换，snapshot metadata、颜色 token、布局、换行和其他正文不变，且没有 `.snap.new`。

`src/tui/mod.rs` 中 session fixture使用的 `"1.2.0"`是历史/任意 metadata测试值，不由 package version驱动，除非测试语义明确要求当前版本，否则不得机械改动。Rust source默认零 diff。

这属于版本文本的既有 UI 输出更新，按设计规范为 `port`；没有新组件、交互或视觉设计。

### D4 · PR 只建立 release candidate，不创建远端发布对象

implementation branch完成 version/docs/spec/snapshot更新并通过本地全量门禁后创建 PR。PR-triggered Release workflow必须验证 synthetic merge ref 的 metadata、Windows/Linux archive和sealed bundle，publish与公开 verify jobs保持 skipped/absent；CI、Security audit与独立审查必须全绿。

PR artifact只用于验证，不能被描述成 GitHub Release。PR provenance以run `event=pull_request`、run `head_sha`、现场synthetic merge revision marker，以及已知PR number对应的持久PR endpoint `.head.sha`共同证明；run对象的`pull_requests[]`只作可选观察值，不得作为长期MUST证据。implementation merge前必须确认远端不存在 `v1.3.0` tag、draft/public Release或同名 assets。

### D5 · tag 只指向完成 master 门禁与 dry-run 的获批 candidate SHA

implementation PR合入后，重新读取唯一`origin/master` tip并把该implementation merge SHA冻结为release candidate；在等待master CI/Security完成或允许其他PR合入前，立即以`ref=master`创建release dry-run并记录run ID，要求创建后的run `head_sha`精确等于candidate。GitHub `workflow_dispatch`接收branch/tag ref而非任意raw commit SHA，因此一旦master先前进就不能事后补建candidate dry-run；只有run identity已锁定后才允许master正常推进。随后等待该精确SHA对应的Windows CI、Ubuntu CI、Security audit以及dry-run metadata、双平台package与aggregate全部成功，publish/public verify不得运行。相同证据还会在tag-triggered publish job首次写Release API前由`actions: read`重新查询；人工tasks与workflow gate互为独立证据，不能互相替代。

在implementation merge前另行取得用户对repository admin mutation的明确授权并分别验证：启用immutable releases；创建/配置受保护`release` environment（reviewer=`tajiaoyezi`、`prevent_self_review=false`、`custom_branch_policies=true`且唯一policy以`name=v1.3.0,type=tag`创建、UI关闭admin bypass）；按D2的完整JSON contract创建active `protect-master` branch ruleset与active `protect-stable-tags` tag ruleset，二者都不配置常驻bypass。每个后续stable release都必须在tag前经同级授权把environment policy更新为自己的精确canonical tag，不用宽泛fnmatch替代workflow SemVer gate。这些设置均生效后才允许merge/tag阶段继续，该授权不等于tag授权。workflow不获取`administration`权限；两组ruleset无常驻bypass、policy `type=tag`、immutable setting与REST未暴露的admin-bypass状态由admin在environment批准前再次人工读取，公开后的`immutable=true`与review history中的实际approval是平台最终机器证据。

展示candidate SHA、checks、用于三类证据的候选`run_id/run_attempt`及其job完成时间、dry-run bundle、version、asset与settings摘要后，必须重新取得用户对“为该精确SHA创建/push annotated `v1.3.0` tag”的明确授权；规划、repository setting或implementation PR的批准均不等于tag授权。创建前后均验证tag object与peeled commit精确等于获批candidate，且candidate仍是受保护`origin/master`可达的ancestor；所有被选证据的预期jobs必须已在tag push前完成。`master`在获批后通过正常PR继续前进不使candidate失效，也不要求移动tag追随新tip。candidate不再可达、ruleset/environment失效或tag ref已存在时必须停止。创建本地annotated tag后、push前还必须读取tag object的tagger timestamp并转换为UTC，要求日期精确等于Changelog heading；若跨UTC日期则不得push，删除未发布的本地tag后走日期修正PR、完整门禁与新的tag授权。

### D6 · 公开 Release 由 tag workflow 原子收口

tag push触发唯一 publish path。metadata、Windows/Linux package、aggregate、publish、Windows/Linux public verify必须全部成功；三个checkout job的revision marker、run head SHA、tag peeled commit与release candidate SHA必须一致。tag run到达`release` environment等待点后，必须向用户展示精确run/candidate、pre-tag evidence及admin settings重读结果，另获并执行本次deployment approval；tag创建授权不包含environment approval，不能复用。metadata checkout证明candidate仍是受保护`master`的ancestor；publish job还必须通过受保护environment、精确Actions/ruleset evidence preflight、sealed-to-remote identity与公开前远端tag重验。GitHub Release对象的`tag_name`必须精确为`v1.3.0`，其revision权威是该tag的远端peeled commit；`target_commitish`只要求存在并记录观察值，不得要求它等于SHA或用它替代peeled tag证据，因为GitHub对已存在tag创建Release时可返回`master`。

公开 Release只含：

- `mysteries-v1.3.0-x86_64-pc-windows-msvc.zip`
- `mysteries-v1.3.0-x86_64-unknown-linux-gnu.tar.gz`
- `SHA256SUMS`

draft公开前再次匿名解析remote tag object/peeled ref并确认等于`REVISION`；公开后Release API必须返回`immutable=true`，证明GitHub已锁定assets与关联tag。自动 verify jobs从匿名 HTTPS重新下载并验证 checksum、文件集、`--version`/`--help`和Linux GLIBC baseline。用户再在 Windows Terminal下载/展开正式 ZIP，启动 TUI并正常退出，确认 PowerShell立即可用；该人工验收完成前 change保持 active。

### D7 · 外部发布证据只在发布后归档，不回写 tag revision

implementation阶段 `tasks.md` 只完成本地/PR相关项；master dry-run、tag、public Release、下载验证、README latest解析与真机项必须保持未完成。`manual-verification.md`只保存可复用procedure/placeholders，不回填会陈旧的远端事实。真实 run/job/release/asset/checksum/真机证据在执行期间汇总，并只在发布后的archive决策记录中持久化；公开验证后先展示post-release README直接链接、sync specs、最终tasks与archive决策记录的精确diff，另获用户对archive branch/commit/push/PR的明确批准后才创建PR，PR checks/review完成后再单独取得merge批准。这样既避免默认分支提前引用不存在的asset，也避免正式archives携带错误发布状态或把archive批准扩张为merge批准。

不为证明发布再向 tag revision追加递归 evidence commit；archive merge发生在 v1.3.0 tag之后并使 `master`领先 tag属于预期。

### D8 · 失败与回滚沿用 fail-closed 边界

- tag前失败：修复 implementation PR或整体 revert；远端不得存在 v1.3.0 tag/Release。
- tag已push但tag workflow任一步失败或被取消：无论是否为零diff/transient、是否已创建draft，`v1.3.0`均立即视为已消耗；保留失败run、tag和残留draft，不得删除、移动、重建、公开或rerun复用。缺失pre-tag evidence、tag前immutable/environment/ruleset未成立、任何批准事实漂移，以及repository/workflow/package/release notes修复一律另起patch release change（通常为v1.3.1）重新版本化并走完整流程。
- Release已公开：v1.3.0 tag/assets不可重写，缺陷只能通过后续 patch release修复。
- tag publish attempt 1执行期间若`master`通过受保护PR正常推进，已获批candidate仍是ancestor且精确证据、immutable/environment/rulesets不变时允许继续；只有candidate不可达、tag漂移或保护失效才fail-closed，失败后仍不得rerun同一tag。

## Risks / Trade-offs

- **[主 spec 泛化时丢失 v1.2.0 历史约束]** → 通用 Requirement保留固定供应链与安全边界；v1.2.0“首个自动化 Release”和历史兼容说明继续留在 Changelog/文档，不作为未来版本的动态要求。
- **[71 份 snapshot 造成大 diff 掩盖视觉回归]** → 逐文件验证只允许 `v1.2.0` → `v1.3.0`，拒绝 metadata/布局/样式/正文变化和 `.snap.new`。
- **[PR checks成功但 tag指向不同 revision]** → implementation merge时冻结candidate SHA，tag授权精确绑定该SHA；workflow内metadata、Actions evidence与远端tag再次校验。
- **[人工漏过、伪造或在tag后补跑既有checks]** → publish以tag run的server-side `created_at`为边界，只选择全部预期jobs更早完成的同SHA attempt并记录精确`run_id/run_attempt`；较晚rerun不能覆盖较早历史，受保护`release` environment另要求平台审批，合法tag本身不再足以获得发布。
- **[重复点击或rerun dry-run造成永久歧义]** → 允许多个合格run/attempt，按完成时间、run ID与attempt确定性选一个；唯一性只约束所选attempt内部的预期job集合。
- **[environment被admin bypass或删除后隐式重建]** → 明确关闭admin bypass、允许单一admin自审但仍要求实际review；publish首次写入前重读environment reviewer/branch policy，缺失环境会显示为空保护并fail-closed。
- **[最终检查与tag push之间master正常推进造成死锁]** → 发布身份绑定已完成门禁的candidate SHA而非瞬时branch tip；active master ruleset阻止force rewrite，candidate仍为ancestor即可继续。
- **[tag在draft公开前被移动/删除]** → active `v*` tag ruleset从创建起禁止更新/删除且无常驻bypass；publish锁内读取ruleset与完整tag refs，公开后再由immutable releases锁定。
- **[draft archives/checksum或Release正文被同时替换]** → remote三文件及API解码后的`.body`分别逐字节等于本run sealed bundle后才可公开，remote checksum自洽或正文“看起来相同”不作为identity证明。
- **[公开后assets或tag被重写]** → tag前启用repository immutable releases；environment reviewer在publish前复核admin事实，公开API必须返回`immutable=true`，否则不得完成change或archive。workflow不为读取setting引入admin token。
- **[implementation merge后README提前产生v1.3.0 404或正式archive仍写Unreleased]** → implementation已把能力标记固化为v1.3.0，但安装命令只使用`releases/latest`动态解析已公开stable；直接v1.3.0历史链接在公开验证后的post-release docs/archive PR更新。
- **[PR run关联数组在merge后消失]** → run `head_sha`加持久PR endpoint `.head.sha`作为权威，`pull_requests[]`只作非阻断观察值。
- **[tag rerun复用run级旧approval或失败后复用版本]** → publish job只允许`run_attempt == 1`；tag一旦push，任何失败均烧掉该version并保留tag/draft证据，后续只走patch release。
- **[动态 workflow实际仍含版本假设]** → implementation阶段用 v1.3.0 PR validation验证真实 archive/bundle；除已确认的publish job四重gate外，若再失败须先确认是contract drift而非绕过门禁，再修订设计后最小修复。
- **[把`target_commitish`误当不可变revision]** → Release metadata只校验其非空并记录；以Release `tag_name`加远端annotated tag peeled commit作为唯一revision证明。
- **[live toolchain/runner/Action环境漂移]** → 重新核验固定 Rust、runner labels与完整 Action SHA仍可用；不可用时修订 spec/design并显式评审，不静默改为 floating `stable`/`*-latest`。
- **[公开后发现 subagent资源风险]** → release notes保留现有明确限制；不在发布 change中声称总token、总扫描量或总内存已有硬上限。

## Migration Plan

1. 在 implementation branch固化 `1.3.0`、Changelog/README、release/dependency-security spec delta、workflow hardening和批准的版本 snapshot。
2. 完成本地 Cargo、RustSec、OpenSpec、snapshot、workflow负向与scope门禁，创建 release candidate PR。
3. 等待 PR CI/Security/Release validation与独立审查全绿；另获授权后启用immutable releases，配置受保护`release` environment及无常驻bypass的active `protect-master`/`protect-stable-tags` rulesets并验证生效；展示checks/review/settings结果并另获“合入精确implementation PR，merge成功后立即以`ref=master`dispatch dry-run”的明确批准后才操作。
4. 将implementation merge SHA冻结为candidate；在等待CI/Security或允许master前进前，立即以`ref=master`dispatch release dry-run并锁定其run ID/head SHA，随后等待该candidate的master CI/Security与dry-run全部完成。
5. 向用户展示精确candidate SHA和证据并取得tag授权；创建本地annotated tag后复核tagger UTC日期及candidate仍可从受保护master到达，再push`v1.3.0`。
6. 验证tag workflow中的Actions/ruleset evidence、sealed/remote identity、公开Release `immutable=true`、Windows/Linux匿名下载与Windows Terminal真机。
7. 公开验证后补充README v1.3.0历史/Release链接并起草archive决策记录；用户批准精确diff后创建post-release docs/archive PR，checks/review全绿且再次批准merge后再合入。

## Open Questions

无。Changelog日期使用annotated tag创建时的UTC日期；若计划日期已过，必须先走日期修正PR和全部精确门禁，禁止在日期不一致的旧revision上创建tag。
