## 1. 发布基线与范围冻结

- [x] 1.1 按 `manual-verification.md` §1 保存当前 package=`1.1.0`、远端无tag/Release、legacy v1.1.0 Windows executable已跟踪的基线；若现场事实漂移，先修订proposal/design/spec再实施。
- [x] 1.2 运行现有CLI version/help定向测试并定位`version_text()`/`CARGO_PKG_VERSION`链，确认版本升级无需修改`src/`；本change不新增headless内核行为，强制TDD接口停点不适用。若实际必须修改Rust运行时，立即停止并重新判定TDD范围。
- [x] 1.3 确认范围只允许新增`release.yml`、修改根package version/lockfile根entry、`CHANGELOG.md`、README、`deliverables/README.md`、本change artifacts，以及仅把既有`src/tui/snapshots/*.snap`中的版本字面量`v1.1.0`精确替换为`v1.2.0`；不得升级dependency、修改MSRV、现有CI/Security、session wire、Rust source、TUI布局/样式或snapshot metadata。
- [x] 1.4 按 `manual-verification.md` §2 从official repositories重新核验`actions/checkout`、`actions/upload-artifact`、`actions/download-artifact`的stable tag、peeled 40位SHA与JavaScript runtime；以design记录的v7.0.0/v7.0.1/v8.0.1 Node.js 24映射为规划基线，现场漂移时以官方事实为准并显式报告，禁止从记忆复制或使用floating ref。
- [x] 1.5 从official Rust release announcement与dist manifest核验并固定release compiler=`rustc 1.96.1 (31fca3adb283cc9dfd56b49cdee9a96eb9c96ffd)`、runner=`windows-2022`/`ubuntu-22.04`与targets=`x86_64-pc-windows-msvc`/`x86_64-unknown-linux-gnu`；不修改全仓`rust-toolchain.toml`或MSRV，只在release workflow显式使用`cargo +1.96.1`。若官方toolchain不可安装或runner label不可用，先修订design/spec而非回退到`stable`/`*-latest`。

## 2. 固化 v1.2.0 版本与发布文档

- [x] 2.1 将`Cargo.toml`根package version改为`1.2.0`，由Cargo更新`Cargo.lock`；审查lockfile diff只改变根`mysteries` version，不执行无界`cargo update`、不改变dependency解析。
- [x] 2.2 将现有`Unreleased`内容固化为`[1.2.0] - <实际发布日期>`，在顶部保留新的空`Unreleased`；显式记录v1.2读取旧session但含`SessionLine::Plan`的新session回退到v1.1.0会失败。
- [x] 2.3 把1.0/1.1标为未创建Git tag/GitHub Release的开发里程碑，分别引用已核验的历史commit而非虚构tag；添加真实`[1.2.0]` release链接与从v1.2.0开始的`[Unreleased]` compare链接。
- [x] 2.4 更新README：优先说明GitHub Release Windows/Linux asset、`SHA256SUMS`、解压、`--version`验证，同时保留源码`cargo build/install --path`路径；刷新与发布直接相关且已漂移的版本/工程事实，不重写无关文案。
- [x] 2.5 更新`deliverables/README.md`，区分历史验证产物与正式Release asset；保留既有v1.1.0 executable且内容/hash不变，不添加任何v1.2.0 binary/archive/checksum blob到Git。

## 3. Release workflow 事件、版本与权限骨架

- [x] 3.1 新增`name: Release`的`.github/workflows/release.yml`，配置release-sensitive `pull_request` paths、永不发布的`workflow_dispatch`与`push.tags: ['v*']`；workflow名称保持稳定，且不得使用`pull_request_target`、branch publish或手动publish input。
- [x] 3.2 设置workflow顶层`permissions: contents: read`；所有validation/package/aggregate/public-smoke jobs保持只读，只有严格tag-gated publish job在job level唯一设置`contents: write`，禁止PAT及其他write/id-token权限。
- [x] 3.3 实现稳定`Validate release metadata` job：使用canonical `^v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$` grammar，拒绝前导零/prerelease/build metadata；tag path fetch完整ref并以`git cat-file -t`拒绝lightweight tag；继续完成event/ref门控、manifest/lockfile根package唯一版本、单一带日期Changelog heading与非空release notes，任一不一致在package/publish前fail-closed。
- [x] 3.4 metadata job输出`version`、`revision`、`publish`与release notes artifact；所有执行checkout的jobs输出恰好一个`RELEASE_REVISION=<40hex>`并断言等于metadata revision。
- [x] 3.5 metadata tag path现场fetch `origin/master`与完整tag ref，验证annotated tag object、tag object SHA、peeled commit精确等于master tip及checkout/run revision；PR/dispatch path强制`publish=false`。该检查是早期门禁，publish job仍须锁内重验，不得放宽为ancestor或把PR head/synthetic merge-ref当release commit。
- [x] 3.6 所有官方Actions使用1.4核验的完整SHA+邻近tag注释，JavaScript runtime受支持；每个checkout均`persist-credentials:false`，不设置不安全runtime override或`continue-on-error`。

## 4. 双平台原生 package validation

- [x] 4.1 在`windows-2022`实现`Package release (windows-x86_64)`：安装/断言Rust`1.96.1` commit与host=`x86_64-pc-windows-msvc`，从同一locked revision执行`cargo +1.96.1 build --release --locked --target x86_64-pc-windows-msvc`，原生运行`--version`/`--help`，构造`mysteries-v1.2.0-x86_64-pc-windows-msvc.zip`并重新展开复核。
- [x] 4.2 在`ubuntu-22.04`实现`Package release (linux-x86_64)`：安装/断言Rust`1.96.1` commit与host/target=`x86_64-unknown-linux-gnu`，从同一locked revision构建/原生smoke；用`readelf`/`objdump`断言ELF required GLIBC symbol version不高于2.35，构造`mysteries-v1.2.0-x86_64-unknown-linux-gnu.tar.gz`，重新展开并验证executable bit。
- [x] 4.3 为Windows/Linux使用互不冲突、含OS/arch的内部artifact名称与有限retention；upload前拒绝空文件、额外根文件、symlink、绝对路径、workspace config/session/credential、`.git`与`target`中间产物。
- [x] 4.4 对打包前binary与重新展开binary分别验证exit 0、版本等于metadata version且`--help`不触网/不读credential；两端archive basename必须包含完整target triple并精确为spec约定，禁止cross compilation替代原生runner证据；README说明GNU/Linux预编译包仅承诺x86_64 glibc 2.35-compatible baseline，更老glibc或musl使用源码构建。

## 5. Bundle、draft 发布与公开后复核

- [x] 5.1 实现只读`Assemble release bundle`：隔离下载两个内部artifacts，拒绝缺失/重复/额外archive、错误version/OS/arch、symlink、空文件和路径逃逸，并再次核对archive根层文件集。
- [x] 5.2 按basename字典序生成UTF-8 Unix格式`SHA256SUMS`，两个archive各恰有一条64hex记录；聚合job本地立即验证checksum，并上传只含两个archive、checksum与release notes的`release-bundle-<version>`。
- [x] 5.3 实现仅合法tag可运行的`Publish GitHub Release`：设置repository/完整tag ref concurrency且`cancel-in-progress:false`；job不checkout、不执行Cargo/仓库脚本/binary，只下载sealed bundle；取得锁后、首次API写入前匿名读取remote master与不带`--refs`的完整tags列表，再按完整ref名本地精确过滤唯一tag object ref与`^{}` peeled ref，要求tag/peeled SHA不同且peeled commit等于metadata/run revision与master tip，再预检同tag draft/public Release及同名asset均不存在。不得把peeled伪ref作为单独remote pattern查询；`GH_TOKEN`只绑定Release API shell steps，不使用clobber/overwrite。
- [x] 5.4 publish job用预装`gh`从已存在远端tag创建stable draft，上传精确三个assets；API回读tag/draft/prerelease/asset集合/size并从draft重新下载验证checksum，全部成立后才转public/latest。
- [x] 5.5 实现`Verify published release (windows-x86_64)`与`Verify published release (linux-x86_64)`只读jobs：不设置`GH_TOKEN`/`GITHUB_TOKEN` env、不调用`gh`或GitHub API，直接从公开HTTPS Release asset URL匿名下载本平台archive+checksum，复核asset、解包文件集、Windows/Linux执行权限、Linux GLIBC baseline、`--version`与`--help`。
- [x] 5.6 实现失败边界：publish前错误不创建draft；创建后错误保留非公开draft且run失败；自动化不得删除/move tag、删除draft/asset或覆盖重试。公开后任何缺陷必须走v1.2.1，不得重写v1.2.0。

## 6. 本地静态与负向验证

- [x] 6.1 按`manual-verification.md` §3验证Cargo/lock/Changelog版本唯一一致、现有CLI version/help测试、release binary `--version`/`--help`；确认Rust source无diff，后续snapshot baseline只接受批准的版本字面量变化。
- [x] 6.2 按§4静态审查event、稳定job名称、publish concurrency、permissions、token input/env边界、所有Action完整SHA/runtime、固定runner/toolchain/target与checkout credential；扫描并拒绝`pull_request_target`、`continue-on-error`、clobber、`*-latest`、浮动`stable`和额外write权限。
- [x] 6.3 在不改真实远端的临时副本/本地命令中覆盖metadata/publish负向case：非canonical tag（含前导零/prerelease/build metadata）、lightweight tag、manifest/lock/Changelog version不一致、重复heading、缺release notes、revision不一致，以及metadata通过后master推进，均在首次Release API写入前失败；模拟相同tag两个publish尝试时只能串行且后者fail-closed。
- [x] 6.4 覆盖bundle负向case：缺/多/重复archive、错误basename、空文件、symlink、额外根文件、checksum缺失/重复/不匹配均fail-closed；验证程序不得删除用户真实文件或访问真实credential。
- [x] 6.5 核对`manual-verification.md`命令、job/asset名称、事件和最终workflow一致；revision验证必须枚举恰好三个允许checkout的job并在marker缺失/重复/错误时失败，不得以step名称缺失作为跳过条件；文件只保留procedure/placeholders，不写实际PR/run/tag/Release值，不形成self-evidence。

## 7. 全量质量、范围与 OpenSpec 门禁

- [x] 7.1 运行`cargo fmt --all -- --check`与`cargo clippy --all-targets --locked -- -D warnings`，全部通过。
- [x] 7.2 运行`cargo test --locked`全量lib/integration tests与`cargo build --release --locked`，记录通过数/ignored；不得用定向版本测试代替全量门禁。
- [x] 7.3 运行固定`cargo-audit audit --deny unsound --file Cargo.lock`，要求0 vulnerability/0 unsound并继续如实展示allowed unmaintained warning；不增加ignore或修改Security policy。
- [x] 7.4 运行`openspec validate release-v1-2-0 --strict`与`openspec validate --all --strict`；既有snapshots只允许版本字面量`v1.1.0` → `v1.2.0`的baseline diff，snapshot metadata与其他渲染正文零diff，`.snap.new`数量0。
- [x] 7.5 运行`git diff --check`、`git diff --name-only`、`git ls-files --others --exclude-standard`与`git status --short`；确认范围只有proposal允许文件（包括批准的版本敏感snapshot baseline）和OpenSpec artifacts，`ci.yml`/`security-audit.yml`逐字不变、无v1.2 binary blob、无凭据/绝对用户路径。

## 8. Implementation PR 验证与合入

- [ ] 8.1 在执行Git写操作前向用户展示最终scope与本地门禁；获批后提交/推送implementation branch并创建PR，记录PR number/head/base，tag与GitHub Release仍不存在。
- [ ] 8.2 按`manual-verification.md` §5验证PR普通Windows/Ubuntu CI、Security audit及release metadata/Windows package/Linux package/aggregate全部成功；release run checkout的是synthetic merge，三个checkout job的revision markers均等于run `head_sha`，Actions run的`pull_requests[].head.sha`等于PR head；publish/public verify jobs skipped/absent。
- [ ] 8.3 下载PR run的内部`release-bundle-1.2.0`，离线验证精确四个文件（两个archives、`SHA256SUMS`与release notes）、checksum与两个archive内容；它只作为workflow artifact，不得被误报为仅含三个公开assets的GitHub Release。
- [ ] 8.4 对implementation code/workflow/docs与OpenSpec做独立审查，无P0/P1/P2后才允许merge；merge前再次确认远端没有`v1.2.0` tag/draft/public Release。

## 9. Master dry-run 与 v1.2.0 tag

- [ ] 9.1 合入implementation PR后，按`manual-verification.md` §6唯一查询精确merge SHA的master CI与Security runs，要求Windows/Ubuntu/RustSec全部success且revision markers等于merge SHA。
- [ ] 9.2 在`origin/master`仍精确等于release merge SHA时触发`release.yml` workflow_dispatch dry-run；metadata/package/aggregate全部success且head SHA一致，publish/public verify不运行，远端仍无tag/Release。
- [ ] 9.3 向用户展示release merge SHA、master门禁、dry-run、version/assets/权限摘要并取得明确tag授权；未获批不得创建或push tag。
- [ ] 9.4 获批后创建annotated`v1.2.0` tag并在push前验证peeled commit精确等于release merge；push后记录tag object SHA、peeled commit与远端ref，禁止移动/覆盖已有tag。

## 10. Tag 发布、公开下载验收与归档

- [ ] 10.1 唯一定位`v1.2.0` tag-triggered release run，验证metadata、Windows/Linux package、aggregate、publish、两个public verify jobs全部success；所有checkout jobs的唯一`RELEASE_REVISION` marker、run `head_sha`与tag peeled commit均等于release merge SHA。
- [ ] 10.2 按`manual-verification.md` §8验证public/latest GitHub Release metadata与精确三个非空assets；draft/prerelease均false，asset names、sizes及API digest（若提供）与workflow证据一致。
- [ ] 10.3 按§9从public Release重新下载Windows ZIP与`SHA256SUMS`，验证checksum/文件集/`--version`/`--help`；在Windows Terminal真机启动并正常退出TUI，PowerShell立即可用且不污染测试外credential/session。
- [ ] 10.4 以tag workflow的Linux public-download job（及可用时的Linux/WSL重复命令）验证Linux tar.gz checksum、文件集、executable bit、`--version`与`--help`。
- [ ] 10.5 重新运行strict OpenSpec与scope检查；依据真实远端证据完成所有tasks，起草`.ai_history/logs/` archive决策记录交用户审阅，内容必须含`manual-verification.md` §10全部字段且不得含凭据/绝对路径。
- [ ] 10.6 用户批准决策记录后，同一archive commit完成`release-delivery`新增主spec、`dependency-security` delta sync、最终tasks勾选与change move；创建archive PR并通过checks后合入。不得追加递归evidence commit，archive后`master`领先`v1.2.0` tag属于预期。
