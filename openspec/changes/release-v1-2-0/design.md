## Context

当前 `master` 的根 package version 为 `1.1.0`，`CHANGELOG.md` 已有一组尚未固化的 `Unreleased` 功能；远端 `gh release list` 与 tag refs 均为空。仓库曾把 `deliverables/mysteries-v1.1.0-windows-x64.exe` 作为人工验证产物提交，但没有 tag、checksums、Linux binary或可重放的发布流水线。Changelog 底部却链接不存在的 `v1.0.0` / `v1.1.0` tag，发布事实源互相矛盾。

现有 CI 已在 Windows/Ubuntu 对每个 PR与 `master` push 执行 fmt、clippy、全量 test和 release build；独立 Security audit 对 vulnerability / unsound fail-closed。所有现有 Action 已固定完整 SHA、使用受支持 runtime，workflow 默认只有 `contents: read`。主 `dependency-security` spec 特意规定未来 publish job 必须通过独立 change 才能扩大权限，因此 release workflow必须在不削弱既有门禁的前提下隔离唯一写权限。

本 change 跨 workflow、版本事实源、package、远端 tag/Release与 OpenSpec archive 时序，且真正发布发生在 implementation PR merge之后。这里的“可复现发布”指每个 asset都能追溯到同一 locked source revision、固定 Rust `1.96.1`、固定 runner OS label、固定 workflow与checksum，并可重新执行同一过程；本 change不承诺不同 runner image patch时间点构建得到 bit-for-bit identical binary。

## Goals / Non-Goals

**Goals:**

- 用一个 change 完成 release automation、v1.2.0版本固化、tag、GitHub Release、下载后验证与归档。
- 让 PR/dry-run先验证双平台 package path，tag path复用同一 build/package逻辑，避免把未经预演的发布脚本直接交给 tag。
- 建立 tag、release commit、Cargo manifest/lockfile、Changelog、binary version、workflow run与GitHub Release assets之间的完整 provenance。
- 只交付 `x86_64-pc-windows-msvc` 与 `x86_64-unknown-linux-gnu`，Linux声明glibc 2.35 baseline，生成并双重验证 `SHA256SUMS`。
- 默认全 workflow只读，仅 tag publish job在最小时间/步骤范围内获得 `contents: write`。
- 在公开前验证完整 draft asset set，在公开后从 Release重新下载并做双平台 checksum/smoke。
- 让历史 1.0/1.1叙述与“从未发布 tag/Release”的真实状态一致，不回写虚构历史。

**Non-Goals:**

- 不发布 macOS、ARM、installer、package manager formula或crates.io crate。
- 不提供代码签名、SBOM、SLSA provenance、GitHub artifact attestation、`id-token`或签名密钥。
- 不引入第三方 release Action、新 Rust dependency或其他实现语言。
- 不追求跨时间/runner bit-for-bit reproducible binary。
- 不删除既有 v1.1.0验证 executable，不重写Git历史，不补建 v1.0/v1.1 tag/Release。
- 不修改运行时代码、CLI grammar、session wire或TUI布局/样式；只允许因package version升级而把既有snapshot baseline中的版本字面量`v1.1.0`精确替换为`v1.2.0`，不得接受其他正文或metadata churn。

## Decisions

### 1. 单一 change 分成 implementation、release、archive 三个远端阶段

同一 OpenSpec change 覆盖完整生命周期，但 Git revision顺序固定为：

```text
implementation PR synthetic merge (绑定该PR head SHA)
        │ PR CI / Security / Release package validation；不得成为release tag
        ▼
release implementation merge on master
        │ exact master CI / Security + workflow_dispatch dry-run
        ▼
annotated v1.2.0 tag (指向该 merge SHA)
        │ tag-triggered package → draft → public → downloaded smoke
        ▼
archive commit / PR (sync specs + tasks 10.x + decision log + move change)
```

tag MUST 指向 implementation merge，而不是后续 archive commit。否则 archive 需要先获得真实 Release证据、Release又要求 tag包含已归档 artifacts，会形成循环。tag tree中 active change仍存在是有意的 OpenSpec时序；archive commit在 Release验收后推进 `master`，不改变既有 tag。

implementation PR只勾选本地/PR阶段 tasks；tag与公开 Release相关 tasks保持未完成。真实 release evidence不回写到 tag revision里的 `manual-verification.md`，而是在 archive时由Agent起草、用户审阅后写入决策记录，并在同一 archive commit中勾选远端 tasks、同步 specs和移动change。这样避免为“证明发布自身”追加递归 evidence carrier。

### 2. 一个 workflow 用事件门控复用同一 package graph

新增 `name: Release` 的 `.github/workflows/release.yml`，稳定workflow与job/check名称：

1. `Validate release metadata`
2. `Package release (windows-x86_64)`
3. `Package release (linux-x86_64)`
4. `Assemble release bundle`
5. `Publish GitHub Release`（仅合法 tag）
6. `Verify published release (windows-x86_64)`（仅 publish后）
7. `Verify published release (linux-x86_64)`（仅 publish后）

触发语义：

- `pull_request`：paths只覆盖 `.github/workflows/release.yml`、`Cargo.toml`、`Cargo.lock`、`CHANGELOG.md`、README、`deliverables/README.md`、LICENSE；运行 1–4，5–7不创建。
- `workflow_dispatch`：只运行 1–4，用于 implementation merge的精确 SHA在打 tag前 dry-run；没有 `publish` input，避免手动事件变成第二条发布入口。
- `push.tags: ['v*']`：metadata先执行 canonical stable SemVer、annotated tag object、版本与provenance验证，通过后运行1–7。

只有`Publish GitHub Release`配置job-level concurrency：group由workflow/repository/完整tag ref组成，`cancel-in-progress: false`。package与aggregate仍可并行；相同tag的远端变更必须串行，后续尝试取得锁后重新预检并fail-closed，不能取消正在创建或校验draft的run。

不让 release workflow成为每个普通源码 PR的重复全量CI；普通代码由既有 CI/Security负责，release-sensitive diff和每个tag才承担package验证。`pull_request` run按GitHub标准语义checkout并验证`refs/pull/<n>/merge` synthetic merge，所有revision marker等于该run `head_sha`；远端验收再通过Actions run的`pull_requests[].head.sha`绑定到目标PR head，不能错误要求run `head_sha`等于PR head。tag创建前的人工门禁仍必须查询 implementation merge的普通CI/Security，而不是把 release workflow green当作替代。

### 3. Metadata job 是唯一版本与 ref 事实源

metadata job在Ubuntu上：

- checkout完整目标 revision，`persist-credentials: false`，输出唯一 `RELEASE_REVISION=$(git rev-parse HEAD)` marker。
- 用 canonical stable SemVer regex `^v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$` 解析tag；PR/dispatch从根 manifest读取当前版本但强制`publish=false`。
- 通过Cargo原生命令读取根 package version并以 `--locked` 验证lockfile；另对根 `Cargo.lock` 的 `mysteries` entry做唯一性/version断言。
- 要求Changelog恰有一个 `## [<version>] - YYYY-MM-DD` heading，并提取其正文为 release notes；必须保留上方新的 `Unreleased` heading。
- tag path现场fetch完整tag ref与`origin/master`，要求`git cat-file -t refs/tags/<tag>`精确为`tag`，拒绝lightweight tag；再验证tag peeled commit精确等于`origin/master`、`GITHUB_SHA`与checkout revision。创建tag前由人工tasks进一步要求它也精确等于已验证implementation merge SHA。metadata只提供早期门禁；publish job取得tag级concurrency锁后、第一次Release API写操作前不建立工作树，而以匿名`git ls-remote --heads`读取master、以不带`--refs`的`git ls-remote --tags`读取全部tag refs，再在本地按完整ref名精确过滤tag object ref与`^{}` peeled ref。两条tag ref必须各一条且SHA不同，peeled SHA必须等于master/run/metadata revision，把metadata后的漂移窗口收窄到首次API写入前；不得把`refs/tags/<tag>^{}`作为单独remote pattern查询，因为Git不会保证返回该peeled伪ref。
- 把version、revision、publish boolean、release notes作为只读 outputs/artifact传给后续jobs，不允许各job重新猜版本。

`Cargo.toml` version修改后使用Cargo更新lockfile，再审查diff只改变根 package version；不能手改dependency解析或执行全量`cargo update`。现有 `version_text()` 使用 `env!("CARGO_PKG_VERSION")`，无需修改Rust代码；既有CLI测试与package smoke足以验证。

### 4. 每个平台原生构建、原生 smoke、单一规范化archive

release构建固定使用Rust `1.96.1`（`rustc` commit `31fca3adb283cc9dfd56b49cdee9a96eb9c96ffd`），不得直接调用浮动`stable`。package jobs都从同一revision在固定runner label执行 `cargo +1.96.1 build --release --locked --target <target>`，不复用仓库已提交binary或其他run的`target/`。每个checkout后输出同一个`RELEASE_REVISION` marker并断言等于metadata output。

产物固定为：

- `mysteries-v1.2.0-x86_64-pc-windows-msvc.zip`
- `mysteries-v1.2.0-x86_64-unknown-linux-gnu.tar.gz`
- 聚合后 `SHA256SUMS`

两个archive的根层只放binary、`LICENSE`、`README.md`。Windows固定`windows-2022`与`x86_64-pc-windows-msvc`，用PowerShell `Compress-Archive`并在原runner重新展开。Linux固定`ubuntu-22.04`与`x86_64-unknown-linux-gnu`；用`tar`且验证executable bit，并以`readelf`/`objdump`从ELF required version信息抽取最高`GLIBC_X.Y`，要求不高于`GLIBC_2.35`。两端都验证runner OS/arch、`rustc +1.96.1 -Vv` host/commit和Cargo target，且在打包前后执行`--version`与`--help`，不得依赖真实provider、credential或network。内部workflow artifact采用不同名称并设置有限retention，避免artifact service把两个平台内容合并到同一目录后发生覆盖。

首版不cross compile，也不承诺musl。README将GNU/Linux预编译包的支持基线写为“x86_64、glibc 2.35-compatible（Ubuntu 22.04或更新兼容环境）”；更老glibc或musl用户使用源码构建，后续再独立规划musl artifact。

### 5. Aggregator先形成封闭bundle，再允许publish job拿写权限

`Assemble release bundle`在Ubuntu只读job中下载两个内部artifacts到隔离目录，执行：

- 预期archive精确集合/数量/非空检查；拒绝symlink、额外archive、重复basename与路径逃逸。
- 展开检查各archive根层文件集；不得把workspace config、session、credential、`.git`或`target`中间产物打包。
- 按filename字典序生成Unix格式`<64hex><two spaces><basename>`的UTF-8 `SHA256SUMS`，本地立即`sha256sum --check`。
- 将两个archives、checksum与metadata生成的release notes上传为单一`release-bundle-<version>`内部artifact。

build/package/aggregate阶段没有GitHub写权限。只有它们成功且event/ref/version门控为publish时，`Publish GitHub Release`才以job-level `contents: write`运行。

### 6. 使用预装 gh 创建draft、API回读、下载验证后公开

不引入第三方release Action；publish job不checkout、不运行Cargo/仓库脚本/release binary，只通过固定SHA的`actions/download-artifact`取得已封闭bundle。它使用GitHub-hosted runner预装`gh`，先输出版本，再通过稳定CLI/REST接口执行；`GH_TOKEN=${{ github.token }}`只绑定需要调用Release API的shell steps，artifact download与本地checksum步骤没有该env。官方checkout在metadata/package jobs可通过默认input使用内建只读token，但全部`persist-credentials:false`；后续build/package步骤没有token env或repository credential。

publish流程：

1. 取得repository/tag concurrency锁；以匿名`git ls-remote --heads`读取`refs/heads/master`，并从不带`--refs`的`git ls-remote --tags`完整输出中本地精确过滤`refs/tags/<tag>`与`refs/tags/<tag>^{}`，要求三者唯一、annotated tag object SHA与peeled SHA不同，peeled commit等于run/metadata revision及master tip。
2. 确认同tag不存在draft/public Release，预期asset名称未占用。
3. 以已存在的远端annotated tag创建stable draft Release，title=`mysteries v<version>`，notes来自Changelog，不自动生成另一份notes。
4. 上传两个archives和`SHA256SUMS`，不使用`--clobber`。
5. API回读tag、draft/prerelease/latest状态、asset精确集合、size>0；从draft重新下载并检查SHA-256。
6. 全部成功才把draft转为public/latest。
7. 只读Windows/Linux verify jobs不设置`GH_TOKEN`/`GITHUB_TOKEN` env、不调用`gh`或API，直接从`https://github.com/<repo>/releases/download/<tag>/<asset>`匿名下载对应archive+checksum，验证并运行`--version`/`--help`，证明public访问而非仅证明token可读。

若draft创建或上传后失败，workflow保留非公开draft供诊断并失败；自动化不删除tag、draft或asset。修复后是否删除未公开draft/tag并重发，必须由用户另行明确批准。Release一旦公开，tag与asset视为不可变；任何缺陷通过v1.2.1修复，禁止重写v1.2.0。

### 7. 发布权限是job级窄例外，不修改既有workflow

workflow顶层`permissions: contents: read`。publish job显式覆盖为仅`contents: write`；所有未列权限保持none。这里区分“job权限”“official checkout的只读token input”和“shell环境显式绑定”：非publish checkout可使用默认只读input但不持久化；publish job不checkout；只有Release API shell steps设置`GH_TOKEN`。禁止`id-token`/attestations/packages/pull-requests/actions/checks/issues写权限、PAT、长期token与`pull_request_target`。

2026-07-12规划现场核验基线为：`actions/checkout v7.0.0` → `9c091bb21b7c1c1d1991bb908d89e4e9dddfe3e0`、`actions/upload-artifact v7.0.1` → `043fb46d1a93c77aae656e7c1c64a875d1fc6a0a`、`actions/download-artifact v8.0.1` → `3e5f45b2cfb9172054b4087a40e8e0b5a5461e7c`，三者官方`action.yml`均声明Node.js 24；Rust official release/dist manifest确认`1.96.1`与rustc commit `31fca3adb283cc9dfd56b49cdee9a96eb9c96ffd`，official runner-images列出`windows-2022`与`ubuntu-22.04`。这些只是规划基线；apply时必须再次从official sources核验，Action tag若漂移则以现场stable tag映射为准并显式报告，但release compiler/runner如需改变必须先修订design/spec，不能静默改用`stable`/`*-latest`。所有`uses:`最终只写完整40位SHA+邻近tag注释，checkout均`persist-credentials:false`，不得设置不安全Node runtime override。`ci.yml`与`security-audit.yml`不修改。

### 8. 版本历史与安装文档以真实远端状态为准

Changelog保留`Unreleased`空入口，将当前内容移到`[1.2.0] - <实际发布日期>`。1.0里程碑以`8c99d0dda69eb5648d7e7ec8871179f73794d439`为历史参考，1.1里程碑以`271d4ee67954dce7ea242144a0268adfd0cd4d61`为参考；两者明确写“开发里程碑，未发布Git tag/GitHub Release”，不创建伪tag链接。`[1.2.0]`才链接真实tag/Release，`[Unreleased]`从v1.2.0开始compare。

README优先提供GitHub Release下载和checksum验证，同时保留源码`cargo build/install --path`路径。`deliverables/README.md`说明目录是演示/历史验证资产，不是当前binary分发渠道；现有v1.1 executable不删除，但禁止添加v1.2 binary。

### 9. 本 change 不触发新的TDD接口停点

实施只改变workflow、根package version、文档与版本敏感的snapshot baseline，不新增或修改headless kernel行为，不需要新增Rust接口RED→GREEN循环。必须先运行既有CLI version/help定向测试锁定动态版本行为，再改version并重新验证；若实现发现必须修改Rust source、session parsing、CLI grammar或TUI布局/样式，则立即停止并先修订proposal/design/spec，重新判断TDD范围。既有snapshots只允许把渲染正文中的`v1.1.0`精确替换为`v1.2.0`，保留原snapshot metadata；不得接受其他正文变化，最终全仓不得残留`.snap.new`。

## Risks / Trade-offs

- **[新workflow在tag path仍可能暴露PR dry-run未覆盖的API问题]** → PR与post-merge dispatch覆盖metadata/package/aggregate；publish先draft并API回读，公开前fail-closed。未公开失败可在人工批准后清理tag/draft重试，公开后只能发patch版本。
- **[GitHub-hosted image patch或预装`gh`漂移]** → 固定runner major label与Rust `1.96.1`，输出image/runner/host/`rustc -Vv`/`gh --version`；固定所有Actions，使用CLI后立即API回读，不把runner环境当无验证事实。
- **[顶层read覆盖与job-level write配置错误]** → 静态解析workflow permissions，PR事件实证publish skipped；tag run记录每个job权限与token仅step-scoped，禁止PAT。
- **[双平台archive内容或文件权限漂移]** → 原生runner打包后立即解包检查；aggregate再次检查，公开后第三次下载验证。
- **[公开后smoke失败]** → build/package阶段已对同一binary原生smoke；公开后失败主要代表上传/下载/metadata问题，workflow保持红且change不得archive，修复按v1.2.1而非覆盖v1.2.0。
- **[metadata通过后master推进或相同tag发布重入]** → publish job以repository/tag concurrency串行且不取消已开始run；取得锁后在第一次Release API写操作前匿名读取remote heads与完整tags列表、再本地精确过滤目标ref，重验annotated tag与master tip，不放宽为ancestor。
- **[“可复现”被误解为bit-identical]** → docs明确承诺locked-source provenance与repeatable process，不承诺跨runner时间bit-for-bit equality。
- **[active OpenSpec change出现在release tag tree]** → 这是为避免release/archive循环的有意时序；archive commit紧随真实release验证，不改变tag或binary。
- **[历史里程碑没有tag导致用户困惑]** → Changelog明确标注并链接历史commit，不伪造release；v1.2.0开始建立规范的tag/Release序列。

## Migration Plan

1. 保存当前无tag/Release、package=`1.1.0`、Changelog/legacy executable基线；验证既有`--version`由`CARGO_PKG_VERSION`驱动。
2. 新增release workflow与静态/本地package验证，核验官方Action tag/SHA/runtime、固定Rust `1.96.1`/runner/target/glibc baseline和permissions；不触发任何远端publish。
3. 将根package升级到`1.2.0`，只允许lockfile根package version变化；固化Changelog并更新README/`deliverables/README.md`。
4. 运行fmt、clippy、全量test、release build、RustSec、OpenSpec和版本文本限定的快照/范围门禁；创建implementation PR，让普通CI/Security与release package validation全部green。
5. merge后查询精确merge SHA的master CI/Security，再从该SHA执行release workflow dispatch dry-run。展示annotated tag目标并取得用户授权后，创建/push`v1.2.0`。
6. 等待tag workflow完成draft→public→双平台downloaded smoke；人工在Windows Terminal下载release ZIP、校验checksum、启动/退出TUI。
7. 依据真实远端证据勾选release tasks，起草用户审阅的archive决策记录；同步delta specs并在同一archive PR中移动change。归档后`master`可领先tag，这是预期状态。

回滚边界：tag前可整体revert implementation merge；tag已push但Release未公开时，自动化不做破坏性清理，必须由用户审查后决定删除draft/tag并在修复后重建；Release公开后禁止重写tag/assets，只能发布v1.2.1并在release notes说明修复。

## Open Questions

- 无阻塞问题。macOS/ARM、crates.io、签名/SBOM/attestation与历史tag治理均保持后续独立change，不影响v1.2.0首版发布边界。
