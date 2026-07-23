## Context

`v1.3.0` tag run `30023496746`、attempt 1 在 candidate `c06cf3b4ecb006003e453beeb6fa0b3f0eb05fc0` 上通过 metadata、双平台 package、aggregate、protected environment approval 与全部远端 evidence preflight。publish 随后成功创建 draft Release `358808315` 并上传三个 sealed assets，但紧接着用 paginated Release list 按 tag 重新发现 ID 时，唯一匹配断言退出 1。失败日志没有打印当时的 match count；随后 API 能唯一读取该 draft，且其正文与三个 assets 均和 sealed bundle 逐字节相等。这个后续状态与 create 后短暂列表不可见假说相容，但不能证明当时断言失败的根因；本change只依赖“create后重新发现ID是不必要的failure window”这一可证事实。

现有 `release-v1-3-0` contract 明确规定 tag push 后任何失败都会消耗该版本：annotated `v1.3.0` tag、失败 run、approval、draft 与 assets 必须保留，禁止 rerun、删除、移动、重建或公开。公开 latest 仍为 `v1.2.0`，修复只能使用新的 `v1.3.1` patch release。

GitHub Create Release REST API 的 `201` 响应已经提供 numeric `.id`、canonical `.url` 与 `.upload_url`；Upload Release Asset API要求使用该 `upload_url`。当前 workflow 丢弃了这份 authoritative identity，先经 `gh release create`/`gh release upload` 写入，再切换到 Release list 重新发现刚创建对象，从而引入没有必要的跨读取路径竞态。

## Goals / Non-Goals

**Goals:**

- 让 draft create、三个 asset upload、draft identity 验证和 public PATCH 全程绑定同一个 Create Release `201` 响应 ID，不再依赖 create 后的列表读后可见。
- 保持 publish fail-closed：create 响应、upload 响应、GET-by-ID、sealed body/assets、tag 或 protection 任一不一致均不得公开。
- 以 `v1.3.1` 重新交付 v1.3 功能集，并如实保留 `v1.3.0` 未公开且已消耗的历史。
- 保持既有 evidence、permissions、immutable release、protected environment、rulesets、anonymous public verification 与失败不可复用边界。

**Non-Goals:**

- 不恢复、rerun、删除、移动或公开 `v1.3.0` tag/draft。
- 不修改任何 Rust runtime source、dependency、config/session wire、CLI/API 或 subagent 行为。
- 不改变 CI/Security workflow、runner/toolchain、Action SHA、GLIBC baseline 或 public asset 文件集。
- 不把 bounded Release-list retry作为主要 identity 机制，也不引入 PAT、长期 credential、`actions: write`、`administration` 或更宽 publish 权限。

## Decisions

### 1. Create Release REST 响应是唯一 draft identity source

publish 在现有 preflight、approval、candidate/tag/evidence/ruleset 重验全部通过后，以 `POST /repos/{owner}/{repo}/releases` 创建 `draft=true`、`prerelease=false` 的 Release。请求正文由 `jq --rawfile` 读取 sealed `release-notes.md` 构造，避免 shell quoting 或换行规范化。

`201` 响应必须在任何 asset upload 前验证：

- `.id` 是正整数；
- `.tag_name == $TAG`、`.name == "mysteries $TAG"`；
- `.draft == true`、`.prerelease == false`；
- JSON 解码后的 `.body` 与 sealed notes 逐字节相等；
- `.url` 精确绑定当前 repository 与该 ID；
- `.upload_url` 精确绑定 `uploads.github.com`、当前 repository 与同一 ID，且只带官方 `{?name,label}` template；
- `.assets` 为空，`.target_commitish` 非空。

验证后把 `release_id` 与 `upload_url` 写入 step outputs；后续不得从 list、tag endpoint、HTML URL 或 branch tip重新推导 ID。

**备选：**在 paginated list 上做 0→1 bounded retry。该方案 diff 更小，但平台没有承诺在任一固定时限内可见，再次超时仍会无谓消耗 patch version，因此拒绝。

**备选：**解析 `gh release create` 输出的 `untagged-*` HTML URL。该 slug不是 numeric API identity，也不是稳定机器契约，因此拒绝。

### 2. 三个 assets 直接上传到 captured `upload_url`

publish 使用官方 Upload Release Asset endpoint逐个上传 Windows ZIP、Linux tar.gz 与 `SHA256SUMS`。asset 名称已经由 canonical version/target grammar约束，无需通用 URL encoder；请求使用 `Content-Type: application/octet-stream`，token仅存在于 upload step，禁止 verbose trace。

每个 `201` upload 响应必须立即验证 positive numeric ID、精确 name、`state=uploaded`、size等于 sealed local file、canonical API URL，并在 API 提供 digest 时与 local SHA-256一致。三个响应 ID必须互异且名称集合精确等于预期集合。任一 upload失败或响应漂移都保留 non-public draft并停止；不得重试 create、使用 `--clobber`、删除 partial assets 或公开部分集合。

**备选：**继续使用 `gh release upload "$TAG"`。它会再次按 tag发现 Release，虽然 `v1.3.0` 现场成功，但仍放弃 captured identity且依赖另一读取路径，因此拒绝。

### 3. 现有验证链改为 GET/PATCH captured ID

upload完成后直接 `GET /releases/{release_id}` 生成 `draft.json`并断言`.id`等于captured Release ID；三个upload `201`响应的asset ID/name/size/API URL/digest tuple必须与该draft `.assets`中的对应tuple精确一一绑定，不能只比较name集合或size。随后继续现有body/assets/checksum逐字节验证、remote tag revalidation与PATCH-by-ID流程。公开后仍通过tag/latest endpoints检查可见性与`immutable=true`，但它们不参与draft identity发现。

create前的 paginated preflight继续检查同 tag不存在任何 draft/public Release；它验证既有状态，不读取刚创建对象，因此没有 create→read竞态。Create POST不得自动 retry；`422`、transport error或非 `201` 均 fail-closed，避免重复创建。

### 4. Patch version与文档保持机械、可审计

根 `Cargo.toml` / `Cargo.lock` 仅把 package version从 `1.3.0` 改为 `1.3.1`。71份 package-version-driven snapshots只接受 `v1.3.0`→`v1.3.1` 字面量变化；Rust source与两个 `src/tui/mod.rs` 历史 `1.2.0` session fixtures保持零 diff。

Changelog保留 `v1.3.0` 条目但明确其 tag/draft保留、未公开且版本已消耗；`v1.3.1` 记录为v1.3功能集的首次公开交付并包含post-create list rediscovery failure window修复，不把eventual-consistency假说写成已证实根因。README把能力版本归属更新到v1.3.1，动态 `releases/latest` 安装块保持不变；`deliverables/README.md`零 diff。

### 5. 远端 release policy只切换精确 patch tag

implementation PR通过后、tag前，把 `release` environment唯一 custom tag policy从 `v1.3.0` 改为 `v1.3.1`。reviewer=`tajiaoyezi`、`prevent_self_review=false`、`can_admins_bypass=false`、custom policy mode、immutable setting以及 `protect-master` / `protect-stable-tags` canonical contract全部保持不变并重新读取验证。

### 6. 失败 change先按真实结果收口

`release-v1-3-0` 只勾选实际完成的 implementation/master/tag/approval步骤与6.6失败分流，成功发布、public smoke和成功归档场景不得伪勾。其通用release contract先sync到主spec，再以 `terminated-by-failure` 决策记录归档；记录 candidate、tag object、run/attempt、draft/assets与失败step，不写credential或本机绝对路径。

## Risks / Trade-offs

- **[Raw REST upload扩大shell实现面]** → 只实现三个固定安全文件名，严格验证create/upload JSON、ID、URL、size与digest，并用fixture覆盖所有failure branches。
- **[Create成功后任一步失败留下partial draft]** → 这是既有fail-closed contract；保留draft作为证据，version立即消耗，禁止自动cleanup或rerun。
- **[Token经curl泄露]** → token只注入create/upload/API steps，使用silent非verbose调用，不打印headers/request，不进入checkout、local checksum或public verify。
- **[直接POST不含`gh release create --verify-tag`]** → 保留且不削弱create前的anonymous annotated-tag/peeled SHA、ruleset、candidate ancestry与preflight evidence重验。
- **[版本机械更新引入snapshot churn]** → 逐文件归一化审计，只接受版本字面量变化，`.snap.new=0`，Rust source零 diff。
- **[v1.3.0残留draft被误操作]** → 所有新请求与environment policy精确绑定 `v1.3.1`；preflight、task与archive log明确禁止触碰旧tag/draft。

## Migration Plan

1. 收口并sync/archive `release-v1-3-0` 的已实现通用spec与失败证据，保持远端对象原状。
2. 先用无credential fixture证明Create response capture与direct upload的正常/失败路径，再修改workflow；完成version/docs/snapshot机械更新。
3. 通过本地全量gates、implementation PR、独立对抗式审查与master dry-run。
4. 更新environment唯一policy为 `v1.3.1`，重读全部settings/rulesets，创建新的annotated tag并取得独立deployment approval。
5. 完成publish、anonymous双平台public verification、Windows Terminal真机、evidence归档。

tag push前可通过受保护PR回滚implementation。`v1.3.1` tag push后不提供原地rollback；任何失败继续保留对象并递增到新的patch version。

## Open Questions

无。
