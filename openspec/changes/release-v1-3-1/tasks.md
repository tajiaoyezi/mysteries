## 1. 冻结 v1.3.0 失败证据

- [x] 1.1 记录并重读 `v1.3.0` candidate、annotated tag object/peeled commit、tag run/attempt、environment approval、失败step、draft Release、三个asset及sealed bundle identity；确认失败日志只证明唯一匹配断言退出，不把eventual-consistency推断写成已证实根因。
- [x] 1.2 通过只读API确认`v1.3.0` tag、失败run、draft、body与assets保持原状，latest仍为`v1.2.0`；禁止rerun、删除、移动、重建、公开或覆盖任一失败对象。
- [x] 1.3 按真实完成状态更新`release-v1-3-0/tasks.md`：只勾选已完成implementation/master/tag/approval步骤与失败分流6.6，保持publish成功、public smoke和成功归档步骤未完成。
- [x] 1.4 选择性sync `release-v1-3-0` 已实现的通用`release-delivery` contract并完整sync `dependency-security` delta，排除未发生的v1.3.0 public/latest成功事实；展示9项未完成tasks、terminated-by-failure spec例外与决策记录精确diff，在本轮用户明确授予的自主授权范围内独立记录archive批准后归档该change，且不得修改远端Release对象。

## 2. v1.3.1 版本与文档

- [ ] 2.1 将根`Cargo.toml`与根package lock entry从`1.3.0`机械更新为`1.3.1`，证明dependency解析零变化。
- [ ] 2.2 更新`CHANGELOG.md`：保留空`Unreleased`，把`v1.3.0`如实标为保留tag/draft、未公开且已消耗，并将v1.3功能集与post-create list rediscovery failure window修复固化到带计划tag UTC日期的`1.3.1`条目；不得把eventual-consistency写成已证实根因。
- [ ] 2.3 更新README中本次能力的版本归属为`v1.3.1`，保持动态`releases/latest`安装命令与既有v1.2.0历史真实；`deliverables/README.md`保持零diff。
- [ ] 2.4 只把71份package-version-driven snapshots中的`v1.3.0`字面量更新为`v1.3.1`；Rust source及两个历史session version fixtures保持零diff，`.snap.new=0`。

## 3. Create响应捕获 TDD

- [ ] 3.1 RED：建立无credential fixture/static assertions，证明现有workflow在Create成功但Release list仍不可见时失败，并覆盖create响应缺失/非法ID、URL/upload URL、tag/name/draft/prerelease/body/assets/target identity漂移、POST冲突与禁止create retry。
- [ ] 3.2 RED：增加direct upload失败矩阵，覆盖非`201`、重复/非法asset ID、错误name/state/size/API URL/digest、同名占用、partial upload，以及upload响应tuple与随后captured Release GET `.assets` tuple串错；确认现有`gh release upload`/list rediscovery实现不能满足这些断言。
- [ ] 3.3 GREEN：把draft创建改为官方Create Release REST POST，使用sealed notes构造请求并验证`201`响应；输出authoritative Release ID、API URL与upload URL，删除create后的list ID rediscovery且不自动retry。
- [ ] 3.4 GREEN：通过captured upload URL上传三个固定assets并逐个验证`201`响应；随后仅以captured ID执行draft GET并断言`.id`一致，把三个upload响应的ID/name/size/API URL/digest tuple与draft `.assets`精确交叉绑定后再做asset下载与public PATCH；禁止`gh release upload`、clobber、list/tag/HTML ID推导。
- [ ] 3.5 运行完整fixture矩阵，增加正向case证明Release list持续不可见时captured ID路径仍成功；所有负向case必须在asset upload或public PATCH的正确边界fail-closed且不访问真实credential。

## 4. 本地质量门与范围审计

- [ ] 4.1 验证metadata、根manifest/lockfile、Changelog唯一heading、README/release notes及release binary version全部为`1.3.1`，lockfile除根version外零diff。
- [ ] 4.2 运行`cargo fmt --all -- --check`、`cargo clippy --all-targets --locked -- -D warnings`、`cargo test --locked`与`cargo build --release --locked`，再验证release binary `--version`/`--help`。
- [ ] 4.3 运行固定`cargo-audit audit --deny unsound --file Cargo.lock`，要求0 vulnerability/0 unsound且不新增ignore。
- [ ] 4.4 运行release metadata/package/evidence/permissions全部既有正负fixture及本change create/upload新增矩阵，确认PR/dispatch不发布、publish不checkout、token scope和job permissions未扩大。
- [ ] 4.5 运行`openspec validate release-v1-3-1 --strict`、`openspec validate --all --strict`、`git diff --check`与scope扫描；确认Rust source、CI/Security workflows、dependency graph、binary/archive/checksum、credential、绝对用户路径及`deliverables/README.md`均零diff。
- [ ] 4.6 独立对抗式审查spec、workflow、fixture、版本文档与snapshot diff；修复所有P0/P1/P2并循环复审至无问题。

## 5. Implementation PR 与远端设置

- [ ] 5.1 创建implementation branch/commit/PR；确认release-sensitive PR触发CI、Security与Release validation，且不创建tag、Release、asset或environment deployment。
- [ ] 5.2 验证PR Windows/Ubuntu CI、RustSec、metadata、双平台package与aggregate全部success，publish/public verify skipped；revision markers、run `head_sha`与PR endpoint `.head.sha`精确一致。
- [ ] 5.3 下载PR `release-bundle-1.3.1`，离线验证四文件sealed bundle、checksums、archive根文件集、binary version/help与release notes identity。
- [ ] 5.4 展示当前environment/settings精确diff，在本轮用户明确授予的自主授权范围内独立记录admin mutation批准后，把`release` environment唯一custom tag policy从`v1.3.0`切换为`name=v1.3.1,type=tag`；重读证明reviewer/self-review/admin-bypass/custom-policy、immutable releases及两个repository rulesets contract均保持批准状态。
- [ ] 5.5 展示implementation PR head、checks、review与merge+dispatch精确对象，在本轮用户明确授予的自主授权范围内独立记录批准后合入PR，冻结唯一merge SHA为candidate，并立即以`ref=master`dispatch release dry-run、锁定其run ID/head SHA；期间不得让未审查commit替换candidate事实。

## 6. Master 门禁与 v1.3.1 tag

- [ ] 6.1 等待candidate的master Windows/Ubuntu CI、RustSec与已锁定dry-run完成，验证精确job集合、attempt-specific完成时间与revision markers；publish/public verify不得运行。
- [ ] 6.2 下载master dry-run bundle，复核version、notes、archive文件集、checksums与binary smoke；确认candidate仍可从受保护master到达且远端不存在`v1.3.1` tag/Release。
- [ ] 6.3 在tag前重读immutable/environment policy/rulesets及全部pre-tag evidence，验证当前UTC日期等于Changelog heading；展示精确candidate/tag对象与失败后版本消耗边界，在本轮用户明确授予的自主授权范围内独立记录tag push批准后创建annotated `v1.3.1` tag，再读tagger UTC日期与peeled candidate，全部一致才push。
- [ ] 6.4 唯一定位attempt 1 tag run并等待`release` environment；展示精确run/candidate/pre-tag evidence，在本轮用户明确授予的自主授权范围内独立记录本次deployment批准后放行，验证approval history精确绑定当前environment/run/reviewer且无admin bypass或非approved冲突记录。

## 7. 发布与公开验证

- [ ] 7.1 验证tag run metadata、双平台package、aggregate、publish与两个public verify jobs全部success；publish日志记录的CI/Security/dry-run attempt、candidate/tag/environment/rulesets及master ancestry证据全部一致。
- [ ] 7.2 验证public Release `tag_name=v1.3.1`、draft/prerelease=false、latest、`immutable=true`、body与sealed notes逐字节相等，远端peeled tag等于candidate；三个remote assets逐字节等于sealed bundle且名称/size/checksum精确。
- [ ] 7.3 匿名下载Windows ZIP与`SHA256SUMS`，验证checksum、根文件集、`mysteries.exe --version == mysteries 1.3.1`及`--help`。
- [ ] 7.4 匿名下载Linux tar.gz与`SHA256SUMS`，验证checksum、根文件集、executable bit、`mysteries --version == mysteries 1.3.1`、`--help`及GLIBC≤2.35。
- [ ] 7.5 在Windows Terminal从public ZIP启动TUI并正常退出，确认PowerShell立即恢复输入且不污染真实credential/session。
- [ ] 7.6 若tag push后任一步失败/取消，立即保留run/tag/draft/assets并把`v1.3.1`视为已消耗；禁止rerun、删除、移动、重建、公开或复用，后续只能另起patch change。

## 8. 证据、复审与归档

- [ ] 8.1 汇总implementation PR、candidate、master gates/dry-run、environment/rulesets、annotated tag、tag run/attempt、Create/Upload captured identity、Release/body/assets/checksums、anonymous smoke与Windows TUI证据；不记录credential或本机绝对路径。
- [ ] 8.2 再次运行全部local/remote/OpenSpec gates与scope审计，并由独立agent对最终代码和证据做对抗式审查；发现问题时修复并循环至无P0/P1/P2。
- [ ] 8.3 起草本change archive决策记录，记录选择captured Create response identity、拒绝list retry/HTML URL/`gh release upload`的理由及最终证据；展示精确archive diff，在本轮用户明确授予的自主授权范围内独立记录决策记录审阅与archive branch/commit/push/PR批准。
- [ ] 8.4 sync delta spec并在同一commit归档change与决策记录，创建archive PR；验证checks与独立复审无问题后再次展示精确PR与merge对象，在本轮用户明确授予的自主授权范围内独立记录最终merge批准后合入，且不得移动已发布tag追随archive commit。
