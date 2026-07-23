## MODIFIED Requirements

### Requirement: Checksums 与 draft-to-public 发布 fail-closed

只读 `Assemble release bundle` job SHALL 从双平台 jobs 下载恰好两个由当前`version`派生的预期 archive，拒绝缺失、重复、额外 archive、symlink 与不匹配 version/target triple 的名称，并生成 UTF-8 `SHA256SUMS`，其中每个 archive 恰有一条 SHA-256 记录；它 MUST 本地验证 checksums，并上传只含两个 archive、`SHA256SUMS`与release notes的sealed bundle。publish job SHALL 只下载该sealed bundle并在无token环境的本地步骤重新验证其精确文件集与checksums；取得受保护environment批准、tag级concurrency锁并完成Actions/rulesets/candidate/tag/provenance重验后，才可创建非 prerelease 的 draft GitHub Release并上传两个 archive与`SHA256SUMS`。

publish job MUST在create前从paginated Release列表按tag证明不存在同tag的draft/public Release；随后通过官方Create Release REST endpoint恰好发起一次`POST /repos/{owner}/{repo}/releases`，请求精确绑定`v<version>` tag、`name="mysteries v<version>"`、sealed release notes、`draft=true`、`prerelease=false`且不得自动retry。create必须返回`201`；workflow MUST直接从同一响应捕获正整数Release ID、canonical API `.url`与官方`.upload_url`，并在任何asset upload前验证响应的`tag_name`、name、draft/prerelease状态、JSON解码后的`.body`、空assets及非空`target_commitish`。`.url`与`.upload_url` MUST精确绑定当前repository和同一Release ID，upload URL除官方`{?name,label}`template外不得含其他template/query。后续步骤 MUST NOT从Release list、tag endpoint、HTML URL或branch tip重新发现或推导该ID；create transport error、非`201`、`422`或任何响应identity漂移都必须fail-closed，且不得重发create。

publish job MUST直接使用captured `.upload_url`逐个上传两个archive与`SHA256SUMS`，每个固定asset name恰好上传一次且不得使用clobber。每个Upload Release Asset `201`响应 MUST立即验证positive numeric asset ID、精确name、`state=uploaded`、size等于sealed local file、canonical API URL；API若提供digest则同时核对本地SHA-256。三个asset ID必须互异且名称集合精确等于预期集合，并保存每个响应的ID/name/size/API URL/digest tuple供后续交叉绑定。任一upload transport/status/identity失败都必须停止并保留non-public draft及已上传assets，不得retry create、删除partial assets、覆盖同名asset或公开部分集合。

三个uploads完成后，publish job MUST只通过`GET /releases/{captured_release_id}`重新读取draft，断言响应`.id`精确等于captured Release ID，并读取`tag_name`、`target_commitish`、draft/prerelease状态、`.body`及assets。draft `.assets`中的ID/name/size/API URL/digest tuple MUST与三个upload `201`响应保存的tuple精确一一对应；asset API URL本身不含Release ID，workflow不得把只匹配name/size的另一组draft assets视为同一上传结果。完成交叉绑定后才可按这些asset ID从`/releases/assets/{asset_id}`下载。remote `SHA256SUMS`与两个remote archives MUST 分别逐字节等于本run sealed bundle中的对应文件，再使用本地sealed manifest验证remote archives；draft API JSON解码后的`.body` MUST逐字节等于sealed `release-notes.md`，不得做换行、空白或编码规范化。只证明remote manifest与remote archives彼此自洽，或只人工观察Release正文语义相似，MUST NOT 被接受为identity证据。API若提供asset digest则同时核对本地hash。`tag_name` MUST 精确等于`v<version>`；`target_commitish` MUST 非空并作为观察值记录，但不得被当作不可变commit SHA或revision权威，因为GitHub对既有tag可返回branch名。发布revision的唯一权威 MUST 是该`tag_name`对应的远端annotated tag peeled commit，并与run/release candidate SHA精确一致。draft阶段MUST NOT依赖仅能解析public Release的`/releases/tags/{tag}`。公开PATCH前 MUST 再次匿名重验remote tag object/peeled ref仍等于run revision。全部成立后必须按同一captured Release ID转为public/latest；公开成功后，Release API的`immutable` MUST 为`true`且public API JSON解码后的`.body`仍逐字节等于sealed notes，再通过tag/latest endpoints复核。

repository immutable releases setting、禁止admin bypass且有实际required review的受保护`release` environment及branch/tag rulesets MUST 在implementation merge/tag之前由已获独立授权的admin操作启用并验证，workflow不得通过PAT、长期credential或扩大`administration`权限自行配置；immutable setting需要admin read，environment reviewer MUST在批准publish前复核该事实，workflow以公开后的`immutable=true`作为平台最终证据。已存在同 tag 的 draft/public Release 或同名 asset 时 MUST fail-closed，不得覆盖；draft创建前失败不得产生Release。tag一旦push，tag workflow任一步失败或取消均使该version立即视为已消耗，不区分零diff/transient，也不取决于candidate、immutable、environment或ruleset是否仍稳定；必须保留失败run、tag及残留draft，不得删除、移动、重建、公开或rerun复用，后续只能走新的patch release change。`master`通过受保护PR正常前进且candidate仍为ancestor不属于attempt 1执行期间的candidate漂移，但失败后仍不得复用该tag。公开后的Windows/Linux verify jobs MUST 不设置`GH_TOKEN`/`GITHUB_TOKEN`环境变量、不调用GitHub CLI/API，而从公开的匿名HTTPS Release asset URL重新下载archive与`SHA256SUMS`，从而证明未登录用户可访问。

#### Scenario: Create响应identity直接驱动后续上传
- **WHEN** Create Release返回唯一合法`201`响应，而create后的paginated Release list仍暂时看不到该draft
- **THEN** workflow MUST使用响应中的Release ID和`upload_url`完成三个uploads并通过`GET /releases/{captured_release_id}`验证draft，MUST NOT等待、重试或依赖Release list重新发现对象

#### Scenario: 非法Create响应在上传前失败
- **WHEN** Create Release响应缺失或给出非法ID/URL/upload URL、错误tag/name/draft/prerelease/body/assets identity、空`target_commitish`或非`201`状态
- **THEN** workflow MUST在第一个asset upload前失败且不得重发create；若平台已经创建draft则保留该non-public对象作为失败证据

#### Scenario: Upload响应漂移阻断公开
- **WHEN** 任一asset upload非`201`，响应中的ID、name、state、size、URL或digest与sealed local asset不一致，或随后captured Release GET的`.id`及asset tuple与create/upload响应不一致
- **THEN** workflow MUST停止且保持draft非公开，不得重建Release、覆盖/删除partial assets或上传后续内容来掩盖失败

#### Scenario: 完整 asset set 与 sealed notes 才可公开
- **WHEN** 当前`version`的两个平台archive、`SHA256SUMS`、captured draft metadata、release body、重新下载的asset与checksum全部匹配
- **THEN** remote三文件与draft API解码后的`.body`分别逐字节等于本run sealed bundle中的对应文件且远端tag再次等于run revision，workflow才把同一captured Release ID转为public/latest；公开Release仅含这三个assets、public `.body`仍等于sealed notes并报告`immutable=true`

#### Scenario: 缺失或额外 asset 及正文漂移阻断公开
- **WHEN** 聚合目录或 draft Release 缺少任一 archive/checksum，出现重复/额外 archive、symlink、错误文件名、空文件、checksum不匹配，或draft/public `.body`与sealed `release-notes.md`存在任一字节差异
- **THEN** workflow MUST 失败；若错误发生在draft创建前则不得产生Release，若draft已创建则保持非公开且不得以部分assets发布

#### Scenario: 既有 tag Release 或Create冲突不被覆盖
- **WHEN** create前发现当前`v<version>`已有draft/public Release、Create POST返回冲突，或任一预期asset名称已占用
- **THEN** workflow MUST 失败并要求人工审查，MUST NOT retry create、使用clobber、覆盖上传或删除既有远端对象

#### Scenario: Tag push 后任一失败都不复用版本
- **WHEN** tag已push后tag workflow任一步失败、取消或尝试以`run_attempt > 1` rerun，无论是否已有draft、是否为零diff/transient以及candidate/immutable/environment/rulesets是否仍稳定
- **THEN** 原version、tag、失败run与残留draft MUST保留为失败证据且不得删除、移动、重建、公开或rerun复用；修复必须使用新的patch version及独立change完成

#### Scenario: 公开 asset 可匿名下载
- **WHEN** draft已转public且Windows/Linux verify jobs开始下载对应archive与`SHA256SUMS`
- **THEN** jobs MUST 通过公开HTTPS URL在没有`GH_TOKEN`/`GITHUB_TOKEN`环境变量及GitHub CLI认证的情况下下载并完成checksum与smoke验证

### Requirement: Release notes 与安装文档如实描述版本历史和兼容性

每次 stable release时，`CHANGELOG.md` SHALL 保留新的空`Unreleased` heading，并把本次内容固化为唯一带annotated tag创建UTC日期的`[<version>]` section；创建本地annotated tag后、push前 MUST 读取实际tagger timestamp并转换为UTC日期再次与heading比较，tag-triggered metadata MUST独立重复相同检查并在不一致时阻断publish。任一tag前检查、本地tag object或workflow机器检查的日期与heading不一致时，维护者 MUST 不push或停止发布；尚未push则移除未发布本地tag并先经新的implementation PR修正日期、重跑该精确revision的全部门禁及重新取得tag授权，已经push则按已消耗version处理，不得给日期不一致的旧revision发布Release。既有版本与链接不得重写。v1.2.0 SHALL 继续标注为首个自动化、可复现的GitHub Release，`1.0.0`/`1.1.0`继续标注为未创建Git tag/GitHub Release的开发里程碑。

根README MUST提供当前stable Windows/Linux Release asset下载、`SHA256SUMS`校验、解压、`--version`验证与源码构建路径。`deliverables/README.md` MUST把该目录与正式分发隔离，标明其中binary仅为历史验证产物，指向GitHub Releases及根README安装说明，并保留源码构建入口；它 MUST NOT重复维护完整下载/checksum/解压命令或把历史binary描述为当前安装源。candidate README MUST把本次已冻结能力标记为待发布version而非`Unreleased`，使正式ZIP/tar.gz内的README准确描述所含功能；安装命令 MUST从公开`releases/latest`解析真实stable tag并由该tag派生versioned asset名称，或在新Release公开前继续固定指向上一个已公开stable，implementation merge MUST NOT提前引用尚不存在的`v<version>` URL。新Release公开且匿名下载验证完成后，维护者 MAY通过post-release docs/archive PR补齐新version直接历史/Release链接；该docs commit MUST NOT移动或重建已发布tag。

v1.3.0 history MUST明确annotated tag、失败run与非公开draft/assets被保留，v1.3.0未成为public/latest Release且不能复用；不得把非公开draft链接或残留assets描述为安装源。v1.3.1 release notes MUST 如实说明Agent execution scope与单层只读`delegate_task`的已交付边界，继续披露没有per-response delegate occurrence硬上限、跨child token总预算、child-only扫描字节硬上限、递归、后台任务、child session及写入/Network child，并记录本change移除了post-create list rediscovery failure window；不得把eventual-consistency假说写成已证实根因，不得把active child≤4或单child 8+1次Provider调用描述为总token、总输出、总扫描量或总内存固定上界，也不得声称存在新的config/session wire迁移。

#### Scenario: v1.3.0 失败历史保持真实
- **WHEN** 阅读v1.3.0版本历史或从安装说明解析当前stable
- **THEN** v1.3.0保留真实annotated tag并明确其attempt 1失败、draft非公开且version已消耗，latest仍指向v1.2.0，任何v1.3.0 draft asset都不是正式安装源

#### Scenario: v1.3.1 首次公开交付 v1.3 能力
- **WHEN** v1.3.1 candidate固化Changelog与README并最终通过public Release验收
- **THEN** execution scope与单层只读委派作为v1.3.1首次公开能力进入带日期历史，release notes同时如实记录post-create list rediscovery window修复与资源/兼容边界，新的Changelog `Unreleased`保持空

#### Scenario: Implementation merge 不提前产生 404
- **WHEN** v1.3.1 implementation PR已合入但tag/Release尚未公开
- **THEN** README可如实标明源码/候选版本为v1.3.1，但动态安装命令仍解析到真实公开的v1.2.0或其他实际latest assets，不引用不存在的public v1.3.1 URL；v1.3.1公开验证后相同命令才自动解析到v1.3.1

#### Scenario: 既有发布历史保持真实
- **WHEN** 阅读版本链接与历史release notes
- **THEN** 1.0/1.1仍是无tag/Release的开发里程碑，v1.2.0仍是首个自动化public Release，v1.3.0明确为保留tag/draft但未公开的失败版本，v1.3.1只在实际public验收后链接自己的真实Release

#### Scenario: 安装说明只引用正式 assets
- **WHEN** 用户按README下载当前stable预编译binary
- **THEN** 下载路径指向该版本public GitHub Release的Windows/Linux versioned archive与`SHA256SUMS`，不把`deliverables/`历史executable或任一non-public draft asset当成当前安装源

### Requirement: stable release 只有在成功验收或失败终止证据完整后才可归档

成功stable release的implementation PR、精确candidate SHA之master CI/Security、同SHA的release dry-run、所选`run_id/run_attempt`、精确repository rulesets、受保护environment审批、publish Actions/ruleset evidence preflight、`v<version>` tag workflow、captured Create/Upload identity、sealed-to-remote asset/body identity、public GitHub Release的`tag_name`/`immutable=true`与远端peeled tag、下载后验证及post-release README状态 SHALL形成同一change的完成证据；`target_commitish`只作为非空观察值，不参与SHA一致性判定。PR长期provenance MUST使用run `head_sha`与已知PR endpoint `.head.sha`，不得依赖可能在merge后为空的run `pull_requests[]`。公开后 MUST在Windows与Linux分别从GitHub Release重新下载对应archive及`SHA256SUMS`，验证checksum、文件集、`--version`与`--help`；Windows Terminal还 MUST真机启动并正常退出TUI，退出后PowerShell输入立即正常。成功链任一步未完成或权威证据指向不同SHA/version时，tasks不得标记为成功完成，change不得以成功发布名义archive。

若tag push后的attempt 1失败或取消且contract禁止复用该version，change MAY以`terminated-by-failure`归档，但必须保留失败run、annotated tag、approval、残留draft/assets原状，勾选失败分流并保持未发生的publish/public/smoke tasks未勾；还必须建立独立patch change、记录失败step与authoritative identity/checksum证据，并让archive决策记录明确“归档不代表Release成功”。该例外不得删除、移动、重建、公开或rerun失败对象，也不得满足任何public release成功Scenario。

#### Scenario: v1.3.1 全链证据一致后允许成功归档
- **WHEN** v1.3.1 implementation PR merge、master CI/Security、dry-run、两个精确rulesets、protected environment、publish Actions/ruleset evidence、tag peeled commit、captured Create/Upload identity、sealed/remote asset与body identity、GitHub Release `immutable=true`及两个下载后binary version全部指向同一candidate，且post-release README与Windows TUI验证完成
- **THEN** change可在记录`run_id/run_attempt`、job/ruleset/environment/release/asset tuple/body identity/checksum/immutable/真机证据并经用户审阅archive决策记录后进入成功归档

#### Scenario: Tag发布失败可按证据终止归档
- **WHEN** tag push后的attempt 1失败，失败version、run、tag与draft/assets均保留，成功发布tasks保持未勾，失败分流已勾且后续patch change已建立
- **THEN** change可在记录失败step、candidate/tag/draft/asset identity与禁止复用决策并经用户审阅后以`terminated-by-failure`带warning归档；该状态MUST NOT声称Release已公开或验收成功

#### Scenario: 公开 Release 不等于自动完成 change
- **WHEN** GitHub Release已公开，但任一下载后checksum/smoke、Windows Terminal真机或证据一致性尚未验证
- **THEN** change MUST保持active，且不得把Release可见性代替最终验收
