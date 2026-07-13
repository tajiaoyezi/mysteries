## ADDED Requirements

### Requirement: Release workflow 严格区分验证与发布事件

仓库 SHALL 提供独立 `.github/workflows/release.yml`。release-sensitive `pull_request` 与 `workflow_dispatch` MUST 只运行 version/package validation，不得创建或修改 tag、GitHub Release 或远端 asset；只有 `push` 一个通过 canonical stable SemVer regex `^v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$` 校验的 annotated tag 才可进入 publish path。workflow MUST NOT 使用 `pull_request_target`，publish job MUST 以事件与 ref 双重条件 fail-closed，未知事件、branch push、带前导零或 prerelease/build metadata 的 tag、lightweight tag及手动 dispatch均不得发布。

#### Scenario: Release-sensitive PR 只验证 package
- **WHEN** pull request 修改 release workflow、根 manifest/lockfile、release notes 或交付文档
- **THEN** Windows/Linux package validation MUST checkout并验证该PR的`refs/pull/<n>/merge` synthetic merge；所有revision markers必须等于现场读取的该merge ref SHA，Actions API的run `head_sha`与`pull_requests[].head.sha`必须等于目标PR head SHA，但publish job不运行，仓库中没有新 tag、draft/public Release 或 Release asset

#### Scenario: 手动 dry-run 永不发布
- **WHEN** 维护者通过 `workflow_dispatch` 在任一 ref 启动 release workflow
- **THEN** workflow 只执行版本与双平台 package validation，任何 publish step 均被事件条件排除

#### Scenario: 非稳定 SemVer tag 被拒绝
- **WHEN** workflow 收到 branch push、`v1.2`、`1.2.0`、`v01.2.0`、`v1.02.0`、`v1.2.0-rc.1`、`v1.2.0+build.1` 或其他不匹配 canonical stable tag grammar 的 ref
- **THEN** publish path MUST 不创建或修改 GitHub Release，并以 skip 或显式失败留下可审计结果

#### Scenario: Lightweight tag 被拒绝
- **WHEN** `refs/tags/v1.2.0` 直接指向 release commit 而不是 annotated tag object
- **THEN** metadata validation MUST 在 package/publish 前失败，即使该 commit、版本与 `origin/master` 完全一致

### Requirement: Tag、源码版本、release notes 与 binary 版本一致

每个可发布 tag MUST 是 annotated tag object并指向创建与tag workflow执行时的精确 `origin/master` tip；创建 tag 前，该精确 commit 的既有 Windows CI、Ubuntu CI 与 Security audit MUST 全部成功。release workflow SHALL 从 tag 名解析唯一版本，并 MUST 断言根 `Cargo.toml` package version、`Cargo.lock` 中根 package version、`CHANGELOG.md` 对应带日期 heading，以及各平台 release binary 的 `mysteries --version` 输出都等于该版本。任一字段缺失、重复或不一致 MUST 在创建 draft Release 前失败。每个执行 checkout 的 release job MUST 输出恰好一个 `RELEASE_REVISION=<40hex>` marker，且同一次 run 的 markers MUST 全部等于 checkout 后的 `git rev-parse HEAD`；tag run 中还 MUST 等于 tag peeled commit、run `head_sha`与workflow现场fetch的`origin/master`。v1.2.0 tag MUST 只在 implementation PR merge 后由已获用户授权的 Git 写操作创建，MUST NOT 在 PR head 或 synthetic merge-ref 上创建。publish job MUST 以repository与完整tag ref为concurrency key串行执行、`cancel-in-progress: false`，并在取得锁后、首次 GitHub Release API 写操作前用匿名`git ls-remote --heads`读取`refs/heads/master`，以不带`--refs`的`git ls-remote --tags`读取tag refs后在本地按完整ref名精确过滤tag object ref与`^{}` peeled ref；tag refs必须精确各一条且SHA不同，以此重验annotated tag、peeled commit、run revision与master tip，任何缺失/重复/漂移 MUST 在创建draft前fail-closed。实现 MUST NOT 依赖把`refs/tags/<tag>^{}`作为单独remote pattern查询。

#### Scenario: 精确 master merge 通过远端门禁后才打 tag
- **WHEN** v1.2.0 implementation PR 已合入，且该 merge SHA 唯一对应的 `master` push CI 与 Security audit 均成功
- **THEN** 维护者可在明确展示 tag target 后创建并 push `v1.2.0`；tag peeled commit 必须等于该 release merge SHA

#### Scenario: PR head 或未验证 commit 不得成为 release tag
- **WHEN** 候选 tag 指向未合入 `master` 的 PR head、synthetic merge-ref，或没有精确成功 CI/Security 证据的 commit
- **THEN** release 操作 MUST 停止且不得 push tag或创建 GitHub Release

#### Scenario: Master 在 tag 之后推进则发布失败
- **WHEN** tag已push，但tag workflow现场fetch到的`origin/master`不再等于tag peeled commit
- **THEN** workflow MUST 在创建draft Release前失败并要求人工判断，MUST NOT因tag commit仍是master ancestor而继续发布

#### Scenario: Metadata 通过后 master 推进仍阻断发布
- **WHEN** metadata job已通过，但 `origin/master` 在publish job取得并发锁前或锁内重验前推进
- **THEN** publish job MUST 在任何Release API写操作前重新匿名读取remote heads与完整tags列表、精确过滤目标refs并失败，远端不得出现draft、asset或public Release

#### Scenario: 同一 tag 的发布尝试串行且不可互相取消
- **WHEN** 同一repository与tag存在两个publish job尝试
- **THEN** concurrency MUST 保证远端变更串行且不取消已开始的publish；后续尝试在锁内预检既有Release或漂移后fail-closed，不得竞争创建或覆盖draft

#### Scenario: 任一版本事实不一致即失败
- **WHEN** tag、`Cargo.toml`、`Cargo.lock`、Changelog heading 或任一 binary `--version` 中至少一个不是同一稳定版本
- **THEN** workflow MUST 在创建 draft Release 前失败，并明确指出不一致的事实源

#### Scenario: Release jobs 测试同一 tag revision
- **WHEN** tag run 的 metadata、Windows package、Linux package、aggregate、publish或公开后verify job执行checkout
- **THEN** 每个相关job日志恰有一个 `RELEASE_REVISION` marker，所有marker与run `head_sha`、tag peeled commit及release merge SHA完全一致

### Requirement: Windows 与 Linux release package 可复现且可独立验证

release workflow SHALL 在 GitHub-hosted `windows-2022` 与 `ubuntu-22.04` 上使用固定 Rust `1.96.1`，分别为 `x86_64-pc-windows-msvc` 与 `x86_64-unknown-linux-gnu` 执行 `cargo +1.96.1 build --release --locked --target <target>`；不得依赖浮动 `stable` 或 `*-latest` 作为release compiler/runner事实源。Linux GNU binary的支持baseline SHALL 为x86_64 glibc 2.35-compatible environment，workflow MUST 从 ELF version requirements计算并断言不存在高于 `GLIBC_2.35` 的required symbol。每个平台 MUST 在原生 runner 上对刚构建的 binary执行 `--version` 与 `--help`，要求exit 0、版本一致且不读取provider credential、不访问网络。产物 MUST 使用含version与完整target triple的唯一名称：Windows ZIP `mysteries-v1.2.0-x86_64-pc-windows-msvc.zip` 含 `mysteries.exe`、`LICENSE`、`README.md`；Linux tar.gz `mysteries-v1.2.0-x86_64-unknown-linux-gnu.tar.gz` 含 `mysteries`、`LICENSE`、`README.md`。archive内不得包含repository credential、config、session、target中间产物或绝对路径。v1.2.0 binary MUST只作为workflow/Release asset交付，不得提交到Git。

#### Scenario: Windows package 原生 smoke
- **WHEN** `windows-2022` release job使用Rust `1.96.1`从已锁定源码构建并展开 `mysteries-v1.2.0-x86_64-pc-windows-msvc.zip`
- **THEN** archive 只含约定文件，`mysteries.exe --version` 精确报告 `1.2.0`，`--help` 成功，workflow artifact 名称不与 Linux 冲突

#### Scenario: Linux package 原生 smoke
- **WHEN** `ubuntu-22.04` release job使用Rust `1.96.1`从已锁定源码构建并展开 `mysteries-v1.2.0-x86_64-unknown-linux-gnu.tar.gz`
- **THEN** archive只含约定文件，`mysteries --version`精确报告`1.2.0`，`--help`成功、executable bit可用，ELF target为x86_64 GNU且required GLIBC symbol version不高于2.35

#### Scenario: 发布产物不回写仓库
- **WHEN** 审查 release implementation diff 与最终 tag tree
- **THEN** 不存在新增 v1.2.0 executable/archive/checksum binary blob；既有 v1.1.0 executable 仅保留为历史验证产物

### Requirement: Checksums 与 draft-to-public 发布 fail-closed

只读 `Assemble release bundle` job SHALL 从双平台 jobs 下载恰好两个预期 archive，拒绝缺失、重复、额外 archive、symlink 与不匹配 version/target triple 的名称，并生成 UTF-8 `SHA256SUMS`，其中每个 archive 恰有一条 SHA-256 记录；它 MUST 本地验证 checksums，并上传只含两个 archive、`SHA256SUMS`与release notes的sealed bundle。publish job SHALL 只下载该sealed bundle并在无token环境的本地步骤重新验证其精确文件集与checksums；取得tag级concurrency锁并完成最终master/tag/provenance重验后，才以 tag 创建非 prerelease 的 draft GitHub Release并上传两个 archive与 `SHA256SUMS`。随后 publish job MUST 通过 GitHub API 重新读取 draft 的 tag、target、draft/prerelease 状态、asset 名称/数量/size，下载 assets 并再次验证 checksums，全部成立后才转为公开。已存在同 tag 的 draft/public Release 或同名 asset 时 MUST fail-closed，不得覆盖；draft创建前失败不得产生Release，draft创建后失败必须保留非公开draft且不得自动清理，残留 draft 只能经明确人工审查处理。失败不得自动删除或移动 tag。公开后的Windows/Linux verify jobs MUST 不设置`GH_TOKEN`/`GITHUB_TOKEN`环境变量、不调用GitHub CLI/API，而从公开的匿名HTTPS Release asset URL重新下载archive与`SHA256SUMS`，从而证明未登录用户可访问。

#### Scenario: 完整 asset set 才可公开
- **WHEN** 两个平台 archive、`SHA256SUMS`、draft metadata、重新下载的 asset 与 checksum 全部匹配
- **THEN** workflow 才把 draft `v1.2.0` Release 转为 public/latest，且公开 Release 仅含这三个 assets

#### Scenario: 缺失或额外 asset 阻断公开
- **WHEN** 聚合目录或 draft Release 缺少任一 archive/checksum，出现重复/额外 archive、symlink、错误文件名、空文件或 checksum 不匹配
- **THEN** workflow MUST 失败；若错误发生在draft创建前则不得产生Release，若draft已创建则保持非公开且不得以部分assets发布

#### Scenario: 既有 tag Release 不被覆盖
- **WHEN** `v1.2.0` 已存在 draft/public Release 或预期 asset 名称已占用
- **THEN** workflow MUST 失败并要求人工审查，MUST NOT 使用 clobber、覆盖上传或删除既有远端对象

#### Scenario: 公开 asset 可匿名下载
- **WHEN** draft已转public且Windows/Linux verify jobs开始下载对应archive与`SHA256SUMS`
- **THEN** jobs MUST 通过公开HTTPS URL在没有`GH_TOKEN`/`GITHUB_TOKEN`环境变量及GitHub CLI认证的情况下下载并完成checksum与smoke验证

### Requirement: Release notes 与安装文档如实描述版本历史和兼容性

`CHANGELOG.md` SHALL 保留新的空 `Unreleased` heading，并把当前条目固化为带日期的 `[1.2.0]`；v1.2.0 notes MUST 明示 `SessionLine::Plan` 的降级不兼容。`1.0.0` / `1.1.0` MUST 标注为未创建 Git tag/GitHub Release 的开发里程碑，MUST NOT 添加虚构历史 tag；v1.2.0 SHALL 标注为首个自动化、可复现的 GitHub Release。README 与 `deliverables/README.md` MUST 提供 Windows/Linux Release asset 下载、`SHA256SUMS` 校验、解压、`--version` 验证与源码构建路径，并区分仓库验证产物和正式 Release asset。

#### Scenario: Changelog 不再链接不存在的历史 tag
- **WHEN** 阅读 v1.2.0 release notes 与底部版本链接
- **THEN** 1.0/1.1 被明确标为开发里程碑且不指向不存在的 tag，v1.2.0 链接到真实 Release/tag，并保留新的 `Unreleased`

#### Scenario: 降级不兼容可见
- **WHEN** 用户查阅 v1.2.0 release notes 或升级说明
- **THEN** 文档明确说明 v1.2 可读旧 session，但含 `Plan` 行的新 session 回退给 v1.1.0 会解析失败，不暗示双向兼容

#### Scenario: 安装说明只引用正式 assets
- **WHEN** 用户按 README 下载预编译 binary
- **THEN** 下载路径指向 GitHub Release 的 Windows/Linux versioned archive 与 `SHA256SUMS`，不把 `deliverables/` 中的历史 executable 当成当前安装源

### Requirement: v1.2.0 只有在公开产物复核后才可归档

release implementation PR、精确 merge SHA 的 master CI/Security、`v1.2.0` tag workflow、public GitHub Release 与下载后验证 SHALL 形成同一 change 的完成证据。公开后 MUST 在 Windows 与 Linux 分别从 GitHub Release 重新下载对应 archive及 `SHA256SUMS`，验证 checksum、文件集、`--version` 与 `--help`；Windows Terminal 还 MUST 真机启动并正常退出 TUI，退出后 PowerShell 输入立即正常。任一步未完成或证据指向不同 SHA/version 时，tasks 与 change MUST 保持未完成，不得 archive。

#### Scenario: 全链证据一致后允许 archive
- **WHEN** implementation PR merge、master CI/Security、tag peeled commit、release workflow、GitHub Release target 与两个下载后 binary version 全部指向同一 v1.2.0 release commit
- **THEN** change 可在记录 run/job/release/asset/checksum/真机证据后进入 archive

#### Scenario: 公开 Release 不等于自动完成 change
- **WHEN** GitHub Release 已公开，但任一下载后 checksum/smoke、Windows Terminal 真机或证据一致性尚未验证
- **THEN** change MUST 保持 active，且不得把 Release 可见性代替最终验收
