## Why

`master` 已完成并归档 v1.3 计划中的 Agent execution scope 与单层只读 `delegate_task`，但根 package、安装文档和公开 Release 仍停留在 `1.2.0`，两项能力仅存在于 `[Unreleased]`。现有 release workflow 已按版本动态构建双平台产物，现在应复用这条已验证的供应链，把同一精确 revision 收口为可下载、可校验且可追溯的 v1.3.0。

## What Changes

- 将根 package 与 lockfile 根 entry 从 `1.2.0` 固化为 `1.3.0`，把当前 `[Unreleased]` 内容发布为带日期的 v1.3.0 release notes，并保留新的空 `[Unreleased]`。
- 把 README 中本次能力从 `[Unreleased]` 固化为 v1.3.0，并把安装命令改为从公开 `releases/latest` 解析实际 stable tag/asset，不在 v1.3.0 尚未公开时提前写入会返回 404 的 URL；公开验证后只在 post-release docs/archive PR 中补齐 v1.3.0 历史/Release链接。保留 v1.2.0 历史记录，不提交 v1.3.0 binary、archive 或 checksum blob。
- 把 `release-delivery` 中仅适用于首次 v1.2.0 的硬编码版本、asset 和归档要求泛化为任意当前 stable SemVer release，同时为 v1.3.0 锁定 implementation PR、master checks、dry-run、annotated tag、GitHub Release、公开下载 smoke 与归档证据链。
- 复用现有 `.github/workflows/release.yml` 的动态版本路径，并加固publish job的event/ref/metadata/首attempt gate、以tag run server time阻止事后补跑的精确revision CI/Security/dry-run证据查询、精确repository ruleset contract、sealed bundle与draft assets/Release body identity校验，以及不可admin bypass的受保护`release` environment；正式tag publish禁止rerun复用旧approval，不改CI/Security workflow权限、package逻辑或Rust构建路径。
- 在tag前经独立授权启用repository immutable releases，配置required reviewer、允许self-review但禁止admin bypass且仅精确`v1.3.0` tag可deploy的受保护`release` environment，配置无常驻bypass、阻止`master`绕过PR/required checks及force rewrite的`protect-master` ruleset，以及禁止`v*` tag更新/删除且无常驻bypass的`protect-stable-tags` ruleset；publish还必须通过review history API证明发生了真实approval。公开后要求Release API返回`immutable=true`，使正式assets与关联tag再由GitHub平台锁定。平台自动生成的release attestation属于该setting的既有效果，本change不自行增加`id-token`、attestation Action或自定义provenance workflow。
- 在 implementation merge 后，选择当时的精确 `origin/master` tip作为不可变release candidate，完成 Windows CI、Ubuntu CI、Security audit和release dry-run后对该获批SHA创建annotated `v1.3.0` tag；后续`master`正常前进不使已验证candidate失效，但candidate必须仍可从受保护`master`到达且tag ref不可更新/删除。tag workflow成功且Windows/Linux公开asset复核通过后才允许归档。
- 本 change 不新增 subagent 行为、资源预算、后台任务、递归、child session、Provider/model policy、平台/架构、installer、代码签名、SBOM、crates.io 发布、自定义SLSA/provenance workflow或 TUI 交互。

## Capabilities

### New Capabilities

无。

### Modified Capabilities

- `release-delivery`: 将 v1.2.0 首发专用要求泛化为可复用的 stable release contract，并规定 v1.3.0 的版本固化、精确 revision、双平台 package、tag/Release 和公开下载归档门禁。
- `dependency-security`: publish job继续独占`contents: write`，仅额外获得`actions: read`以查询同一revision且在tag run创建前已完成的CI/Security/dry-run证据，并只读复核environment/rulesets；该token不得进入checkout、仓库代码、构建、bundle校验或公开下载。

## Impact

- 版本与文档：`Cargo.toml`、`Cargo.lock`、`CHANGELOG.md`、`README.md`，以及仅包含版本字面量的既有 TUI snapshot（若实际存在）。
- 发布契约与流程：`openspec/specs/release-delivery/spec.md`、`openspec/specs/dependency-security/spec.md`、本 change artifacts，以及`.github/workflows/release.yml`中publish job的门禁、只读Actions证据查询、remote asset identity与immutable结果校验。
- 远端状态：受保护`release` environment、repository immutable releases setting、active `protect-master`与`protect-stable-tags` rulesets、implementation PR 合入后的master checks、手动dry-run、annotated `v1.3.0` tag、GitHub Release与公开assets。repository settings、implementation merge+dry-run dispatch与tag分别需要新的明确授权。
- 不新增 dependency，不修改 Rust runtime/API、Provider/Tool/Permission/session wire、TUI 布局或 theme。
