# v1.2.0 Release 手工与远端验证

本文件是静态验证程序，不在 implementation/tag 之后回填真实 SHA、run ID或Release字段。所有实际值由archive阶段从Git/GitHub重新查询，写入用户审阅的archive决策记录；不得追加“证明自身”的evidence commit。

## 0. 变量与不变量

```powershell
$Repo = 'tajiaoyezi/mysteries'
$Version = '1.2.0'
$Tag = "v$Version"
$ExpectedAssets = @(
    "mysteries-v$Version-x86_64-pc-windows-msvc.zip",
    "mysteries-v$Version-x86_64-unknown-linux-gnu.tar.gz",
    'SHA256SUMS'
)
$Change = 'release-v1-2-0'

function Get-ExactRemoteTagRefs {
    param([string]$Remote, [string]$TagName)
    $Lines = @(git ls-remote --tags $Remote)
    if ($LASTEXITCODE -ne 0) { throw "读取remote tags失败: $Remote" }
    $EscapedTag = [regex]::Escape($TagName)
    @($Lines | Where-Object { $_ -match "\srefs/tags/$EscapedTag(?:\^\{\})?$" })
}
```

始终成立：

- release tag只能指向implementation merge，不指向PR head/synthetic merge/archive commit；
- PR与`workflow_dispatch`只验证，不发布；
- tag workflow的所有revision markers、run `head_sha`、tag peeled commit与Release target必须相同；
- `Cargo.toml`、`Cargo.lock`、Changelog heading、两个binary `--version`都为`1.2.0`；
- release compiler固定为Rust `1.96.1`，runner固定为`windows-2022`/`ubuntu-22.04`，Linux asset为`x86_64-unknown-linux-gnu`且required GLIBC symbol version不高于2.35；
- publish job按repository/tag串行，在锁内首次API写入前重验annotated tag与`origin/master`；
- public verify不带token、不调用`gh`，直接匿名下载公开asset；
- v1.2.0公开后不覆盖tag/assets，修复只能发patch版本；
- archive前才把真实证据写入archive决策记录。

## 1. 规划/实施前基线

```powershell
git status --short
git branch --show-current
git rev-parse HEAD

(cargo metadata --locked --no-deps --format-version 1 | ConvertFrom-Json).packages |
    Where-Object name -eq 'mysteries' |
    Select-Object name,version

gh release list --limit 20 --json tagName,name,isDraft,isPrerelease,publishedAt
git ls-remote --tags --refs origin
Get-Item -LiteralPath 'deliverables\mysteries-v1.1.0-windows-x64.exe' |
    Select-Object Name,Length
```

基线必须记录：package=`1.1.0`、远端release/tag为空、legacy executable已跟踪。若现场事实漂移，先按远端/Git/Cargo事实修订规划，不静默沿用。

## 2. 官方 Action tag/SHA/runtime 核验

实施时为`actions/checkout`、`actions/upload-artifact`、`actions/download-artifact`选择官方stable release；不得从记忆复制版本。对每个Action执行：

```powershell
$ActionRepo = 'actions/<name>'
$ActionTag = 'v<major.minor.patch>'
$Refs = @(Get-ExactRemoteTagRefs "https://github.com/$ActionRepo.git" $ActionTag)
$EscapedActionTag = [regex]::Escape($ActionTag)
$DirectRefs = @($Refs | Where-Object { $_ -match "\srefs/tags/$EscapedActionTag$" })
$PeeledRefs = @($Refs | Where-Object { $_ -match "\srefs/tags/$EscapedActionTag\^\{\}$" })
if ($DirectRefs.Count -ne 1 -or $PeeledRefs.Count -gt 1) { throw "官方tag缺失或重复: $ActionRepo@$ActionTag" }
$Refs

# annotated tag优先使用^{} peeled commit；否则使用tag自身commit。
$SelectedRef = if ($PeeledRefs.Count -eq 1) { $PeeledRefs[0] } else { $DirectRefs[0] }
$ActionSha = ($SelectedRef -split '\s+')[0]
if ($ActionSha -notmatch '^[0-9a-f]{40}$') { throw 'Action SHA不是完整40位' }

$ApiPath = "repos/$ActionRepo/contents/action.yml?ref=$ActionSha"
$Encoded = (gh api $ApiPath | ConvertFrom-Json).content -replace '\s',''
$ActionYaml = [Text.Encoding]::UTF8.GetString([Convert]::FromBase64String($Encoded))
$ActionYaml
```

JavaScript Action的`runs.using`必须是当前受支持runtime；release workflow中必须写`uses: ...@<40hex> # <tag>`。记录核验日期、tag、peeled SHA与runtime。不得设置`ACTIONS_ALLOW_USE_UNSECURE_NODE_VERSION`。

同一阶段从Rust official dist manifest核验固定release compiler，并从official runner-images README确认固定runner labels仍受支持：

```powershell
$RustVersion = '1.96.1'
$RustCommit = '31fca3adb283cc9dfd56b49cdee9a96eb9c96ffd'
$RustResponse = Invoke-WebRequest -UseBasicParsing "https://static.rust-lang.org/dist/channel-rust-$RustVersion.toml"
$RustManifest = if ($RustResponse.Content -is [byte[]]) {
    [Text.Encoding]::UTF8.GetString($RustResponse.Content)
} else {
    [string]$RustResponse.Content
}
$RustPattern = '(?ms)^\[pkg\.rustc\]\s+version = "{0} \([0-9a-f]+ [0-9-]+\)"\s+git_commit_hash = "{1}"' -f
    [regex]::Escape($RustVersion), $RustCommit
if ($RustManifest -notmatch $RustPattern) {
    throw 'official Rust dist manifest与固定release compiler不一致'
}

$RunnerResponse = Invoke-WebRequest -UseBasicParsing 'https://raw.githubusercontent.com/actions/runner-images/main/README.md'
$RunnerReadme = if ($RunnerResponse.Content -is [byte[]]) {
    [Text.Encoding]::UTF8.GetString($RunnerResponse.Content)
} else {
    [string]$RunnerResponse.Content
}
foreach ($Label in @('windows-2022','ubuntu-22.04')) {
    if (-not $RunnerReadme.Contains("``$Label``")) { throw "official runner label不可用: $Label" }
}
```

## 3. 本地版本、文档与Rust回归

```powershell
$Metadata = cargo metadata --locked --no-deps --format-version 1 | ConvertFrom-Json
$Root = @($Metadata.packages | Where-Object name -eq 'mysteries')
if ($Root.Count -ne 1 -or $Root[0].version -ne $Version) { throw 'Cargo.toml版本不一致' }

cargo tree --locked -p "mysteries@$Version" --depth 0
if ($LASTEXITCODE -ne 0) { throw 'Cargo.lock根package版本不一致' }

$Changelog = Get-Content -LiteralPath 'CHANGELOG.md' -Raw
if (([regex]::Matches($Changelog, "(?m)^## \[$([regex]::Escape($Version))\] - \d{4}-\d{2}-\d{2}$")).Count -ne 1) {
    throw 'v1.2.0 Changelog heading缺失或重复'
}
if (([regex]::Matches($Changelog, '(?m)^## \[Unreleased\]$')).Count -ne 1) {
    throw 'Unreleased heading缺失或重复'
}

cargo test --lib version
cargo fmt --all -- --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
cargo build --release --locked
& '.\target\release\mysteries.exe' --version
& '.\target\release\mysteries.exe' --help

openspec validate $Change --strict
openspec validate --all --strict
git diff --check
```

要求：`--version`精确为`mysteries 1.2.0`（以实际既有格式为准但版本必须唯一），`--help` exit 0；既有snapshots只允许渲染正文中的版本字面量`v1.1.0`精确替换为`v1.2.0`，原snapshot metadata与其他正文零diff，全仓`.snap.new`为0；`src/`除这些版本敏感的`src/tui/snapshots/*.snap` baseline外无变更，Rust source零diff。

release workflow另须安装并输出`rustc +1.96.1 --version --verbose`，commit hash必须为`31fca3adb283cc9dfd56b49cdee9a96eb9c96ffd`；不得把本地浮动`stable`结果当release compiler证据。

## 4. release.yml 静态边界

```powershell
$Workflow = Get-Content -LiteralPath '.github\workflows\release.yml' -Raw

@(
  'pull_request', 'workflow_dispatch', 'push', 'tags',
  'concurrency:', 'cancel-in-progress: false',
  'Validate release metadata',
  'Package release (windows-x86_64)',
  'Package release (linux-x86_64)',
  'Assemble release bundle',
  'Publish GitHub Release',
  'Verify published release (windows-x86_64)',
  'Verify published release (linux-x86_64)',
  'permissions:', 'contents: read', 'contents: write',
  'persist-credentials: false', 'RELEASE_REVISION=',
  'cargo +1.96.1 build --release --locked',
  'windows-2022', 'ubuntu-22.04',
  'x86_64-pc-windows-msvc', 'x86_64-unknown-linux-gnu',
  'GLIBC_2.35', 'SHA256SUMS'
) | ForEach-Object {
    if (-not $Workflow.Contains($_)) { throw "release.yml缺少: $_" }
}

$Forbidden = @(
  'pull_request_target', 'continue-on-error', '--clobber',
  'ACTIONS_ALLOW_USE_UNSECURE_NODE_VERSION',
  'windows-latest', 'ubuntu-latest',
  'id-token: write', 'packages: write', 'pull-requests: write',
  'actions: write', 'checks: write', 'issues: write'
)
$Forbidden | ForEach-Object {
    if ($Workflow.Contains($_)) { throw "release.yml包含禁止项: $_" }
}

$Uses = [regex]::Matches($Workflow, '(?m)^\s*-?\s*uses:\s*([^#\r\n]+)')
foreach ($Use in $Uses) {
    $Ref = $Use.Groups[1].Value.Trim()
    if ($Ref -notmatch '@[0-9a-f]{40}$') { throw "Action未固定完整SHA: $Ref" }
}

git diff --exit-code -- '.github/workflows/ci.yml' '.github/workflows/security-audit.yml'
```

再人工逐job审查：workflow顶层read；只有tag-gated publish job为`contents: write`且配置repository/tag concurrency、`cancel-in-progress:false`；publish job没有checkout/Cargo/仓库脚本/binary执行，并在首次API写入前匿名读取remote master与不带`--refs`的tags输出、按完整ref名本地精确过滤唯一tag object/peeled refs，验证tag/peeled SHA不同且peeled等于master/run/metadata revision；`GH_TOKEN`仅出现在Release API shell steps。只有metadata/Windows package/Linux package三个job执行checkout，均只用内建只读token input且不持久化credential；build/package/checksum/smoke无token env。两个public verify jobs只能使用匿名HTTPS URL，不能调用`gh`/API。

## 5. Implementation PR 门禁

```powershell
$Pr = 0 # 替换为implementation PR number
if ($Pr -le 0) { throw '请先填写implementation PR number' }
$PrInfo = gh pr view $Pr --json number,state,headRefName,headRefOid,baseRefName,baseRefOid,mergeable,url | ConvertFrom-Json
$PrInfo | Format-List
if ($PrInfo.state -ne 'OPEN' -or $PrInfo.baseRefName -ne 'master') { throw 'PR状态或base异常' }

gh pr checks $Pr

$ReleaseRuns = @(gh run list --event pull_request --branch $PrInfo.headRefName --limit 50 --json databaseId,workflowName,headSha,event,status,conclusion,attempt,url | ConvertFrom-Json)
$ReleaseRuns = @($ReleaseRuns | Where-Object {
    $_.workflowName -eq 'Release' -and $_.headSha -eq $PrInfo.headRefOid
})
if ($ReleaseRuns.Count -ne 1 -or $ReleaseRuns[0].conclusion -ne 'success') {
    throw '当前PR head的release validation run不唯一或未成功'
}
$ReleaseRun = $ReleaseRuns[0]
$ReleaseRunApi = gh api "repos/$Repo/actions/runs/$($ReleaseRun.databaseId)" | ConvertFrom-Json
$RunPullRequests = @($ReleaseRunApi.pull_requests)
if ($ReleaseRunApi.head_sha -ne $ReleaseRun.headSha -or $ReleaseRun.headSha -ne $PrInfo.headRefOid -or $RunPullRequests.Count -ne 1) {
    throw 'PR release run head_sha或PR关联异常'
}
if ($RunPullRequests[0].number -ne $Pr -or $RunPullRequests[0].head.sha -ne $PrInfo.headRefOid -or $RunPullRequests[0].base.ref -ne 'master' -or $RunPullRequests[0].base.sha -ne $PrInfo.baseRefOid) {
    throw 'PR release run没有绑定目标PR head/base'
}
$MergeRefs = @(git ls-remote --refs origin "refs/pull/$Pr/merge")
if ($LASTEXITCODE -ne 0 -or $MergeRefs.Count -ne 1) { throw '无法唯一读取当前PR synthetic merge ref' }
$MergeParts = @($MergeRefs[0] -split '\s+')
if ($MergeParts.Count -ne 2 -or $MergeParts[0] -notmatch '^[0-9a-f]{40}$' -or $MergeParts[1] -ne "refs/pull/$Pr/merge") {
    throw '当前PR synthetic merge ref格式异常'
}
$SyntheticMergeSha = $MergeParts[0]
$ReleaseRunDetail = gh run view $ReleaseRun.databaseId --attempt $ReleaseRun.attempt --json jobs | ConvertFrom-Json
$ExpectedRevisionJobs = @(
    'Validate release metadata',
    'Package release (windows-x86_64)',
    'Package release (linux-x86_64)'
)
foreach ($ExpectedJobName in $ExpectedRevisionJobs) {
    $Jobs = @($ReleaseRunDetail.jobs | Where-Object name -eq $ExpectedJobName)
    if ($Jobs.Count -ne 1 -or $Jobs[0].conclusion -ne 'success') {
        throw "PR checkout job缺失、重复或未成功: $ExpectedJobName"
    }
    $Log = @(gh run view $ReleaseRun.databaseId --attempt $ReleaseRun.attempt --job $Jobs[0].databaseId --log)
    if ($LASTEXITCODE -ne 0 -or $Log.Count -eq 0) { throw "无法读取PR job log: $ExpectedJobName" }
    $Matches = [regex]::Matches(($Log -join "`n"), 'RELEASE_REVISION=([0-9a-f]{40})')
    if ($Matches.Count -ne 1 -or $Matches[0].Groups[1].Value -ne $SyntheticMergeSha) {
        throw "PR revision marker缺失、重复或不等于synthetic merge: $ExpectedJobName"
    }
}

if (@(Get-ExactRemoteTagRefs origin $Tag).Count -ne 0) { throw 'PR阶段不应存在v1.2.0 tag' }
gh release view $Tag *> $null
if ($LASTEXITCODE -eq 0) { throw 'PR阶段不应存在v1.2.0 Release' }
```

要求普通Windows/Ubuntu CI、Security audit和release metadata/package/aggregate全部成功；publish/verify-public jobs必须skipped/absent。下载PR run的`release-bundle-1.2.0`内部artifact，要求精确四个文件（两个archives、`SHA256SUMS`与release notes）、checksum通过；其中只有前3个文件会成为公开assets，它本身不是GitHub Release。

## 6. Merge 后 master 与 dispatch dry-run

```powershell
$PrMerged = gh pr view $Pr --json state,mergeCommit,headRefOid,url | ConvertFrom-Json
if ($PrMerged.state -ne 'MERGED') { throw 'implementation PR尚未合入' }
$ReleaseMergeSha = $PrMerged.mergeCommit.oid

$MasterRuns = gh run list --commit $ReleaseMergeSha --event push --limit 20 --json databaseId,workflowName,headSha,event,status,conclusion,attempt,url | ConvertFrom-Json
$MasterRuns | Format-Table workflowName,databaseId,attempt,status,conclusion,headSha

# 必须唯一找到CI与Security audit，两个run的headSha均等于ReleaseMergeSha且success。

gh workflow run release.yml --ref master
# 等待新workflow_dispatch run后记录ID：
$DryRunId = 0 # 替换为workflow_dispatch run ID
if ($DryRunId -le 0) { throw '请先填写workflow_dispatch run ID' }
$Dry = gh run view $DryRunId --json headSha,event,status,conclusion,attempt,jobs,url | ConvertFrom-Json
if ($Dry.event -ne 'workflow_dispatch' -or $Dry.headSha -ne $ReleaseMergeSha -or $Dry.conclusion -ne 'success') {
    throw 'post-merge dry-run未验证精确release merge'
}

if (@(Get-ExactRemoteTagRefs origin $Tag).Count -ne 0) { throw 'dry-run前不得存在tag' }
gh release view $Tag *> $null
if ($LASTEXITCODE -eq 0) { throw 'dry-run不得创建Release' }
```

dispatch run只允许metadata/package/aggregate成功，publish/public verify不得运行。若`master`在merge与dispatch之间前进，停止并重新决定release commit，不得把不同SHA的dry-run当证据。

## 7. Annotated tag 授权与创建

此节是Git/远端写操作，执行前必须向用户展示`$ReleaseMergeSha`、普通CI/Security与dry-run结论，并取得明确批准。

```powershell
git fetch origin master --tags
if ((git rev-parse origin/master) -ne $ReleaseMergeSha) { throw 'origin/master已不是release merge；停止打tag' }
if (@(Get-ExactRemoteTagRefs origin $Tag).Count -ne 0) { throw '远端tag已存在' }

# 获批后才执行：
git tag -a $Tag $ReleaseMergeSha -m "mysteries $Tag"
if ((git cat-file -t "refs/tags/$Tag") -ne 'tag') { throw '本地tag不是annotated tag object' }
if ((git rev-list -n 1 $Tag) -ne $ReleaseMergeSha) { throw '本地tag peeled commit异常' }
git show --no-patch --format=fuller $Tag
git push origin "refs/tags/$Tag"

$PushedTagRefs = @(Get-ExactRemoteTagRefs origin $Tag)
if ($PushedTagRefs.Count -ne 2) { throw 'push后远端annotated tag refs不完整' }
$PushedTagRefs
```

记录tag object SHA、peeled commit与push时间；peeled commit必须精确等于implementation merge。

## 8. Tag workflow 与公开 Release

```powershell
$RemoteMaster = ((git ls-remote --heads origin 'refs/heads/master') -split '\s+')[0]
if ($RemoteMaster -ne $ReleaseMergeSha) { throw 'tag workflow期间origin/master已推进；发布应fail-closed' }
$RemoteTagRefs = @(Get-ExactRemoteTagRefs origin $Tag)
if ($RemoteTagRefs.Count -ne 2) { throw '远端annotated tag object/peeled refs缺失或重复' }
$RemoteTagObject = (($RemoteTagRefs | Where-Object { $_ -match "refs/tags/$([regex]::Escape($Tag))$" }) -split '\s+')[0]
$RemoteTagPeeled = (($RemoteTagRefs | Where-Object { $_ -match "refs/tags/$([regex]::Escape($Tag))\^\{\}$" }) -split '\s+')[0]
if ($RemoteTagObject -eq $RemoteTagPeeled -or $RemoteTagPeeled -ne $ReleaseMergeSha) {
    throw '远端tag不是预期annotated tag或peeled commit异常'
}

$TagRuns = @(gh run list --event push --commit $ReleaseMergeSha --limit 50 --json databaseId,workflowName,headSha,event,status,conclusion,attempt,url | ConvertFrom-Json | Where-Object workflowName -eq 'Release')
$TagRuns | Format-Table
if ($TagRuns.Count -ne 1) { throw 'tag-triggered Release run不唯一' }
$TagRunId = $TagRuns[0].databaseId
$TagAttempt = $TagRuns[0].attempt
$TagRun = gh run view $TagRunId --attempt $TagAttempt --json headSha,event,status,conclusion,attempt,jobs,url | ConvertFrom-Json
if ($TagRun.headSha -ne $ReleaseMergeSha -or $TagRun.event -ne 'push' -or $TagRun.conclusion -ne 'success') {
    throw 'tag-triggered Release run revision/event/conclusion异常'
}
$TagRun.jobs | Select-Object name,databaseId,status,conclusion,url | Format-Table

# 只有以下三个job允许checkout；逐个要求job唯一、成功且RELEASE_REVISION恰好出现一次。
$ExpectedRevisionJobs = @(
    'Validate release metadata',
    'Package release (windows-x86_64)',
    'Package release (linux-x86_64)'
)
foreach ($ExpectedJobName in $ExpectedRevisionJobs) {
    $Jobs = @($TagRun.jobs | Where-Object name -eq $ExpectedJobName)
    if ($Jobs.Count -ne 1 -or $Jobs[0].conclusion -ne 'success') {
        throw "checkout job缺失、重复或未成功: $ExpectedJobName"
    }
    $Job = $Jobs[0]
    $Log = @(gh run view $TagRunId --attempt $TagAttempt --job $Job.databaseId --log)
    if ($LASTEXITCODE -ne 0 -or $Log.Count -eq 0) { throw "无法读取job log: $ExpectedJobName" }
    $Matches = [regex]::Matches(($Log -join "`n"), 'RELEASE_REVISION=([0-9a-f]{40})')
    if ($Matches.Count -ne 1 -or $Matches[0].Groups[1].Value -ne $ReleaseMergeSha) {
        throw "revision marker缺失、重复或错误: $ExpectedJobName"
    }
}

$Release = gh release view $Tag --json tagName,name,isDraft,isPrerelease,publishedAt,url,assets | ConvertFrom-Json
$LatestEntry = @(gh release list --limit 20 --json tagName,isLatest,isDraft,isPrerelease,publishedAt | ConvertFrom-Json | Where-Object tagName -eq $Tag)
$Release | Format-List tagName,name,isDraft,isPrerelease,publishedAt,url
if ($LatestEntry.Count -ne 1) { throw 'release list中v1.2.0不唯一' }
if ($Release.tagName -ne $Tag -or $Release.isDraft -or $Release.isPrerelease -or -not $LatestEntry[0].isLatest) {
    throw '公开Release metadata异常'
}
$Names = @($Release.assets | ForEach-Object name | Sort-Object)
if (Compare-Object ($ExpectedAssets | Sort-Object) $Names) { throw 'Release asset集合异常' }
if (@($Release.assets | Where-Object size -le 0).Count -ne 0) { throw 'Release含空asset' }
```

要求tag workflow所有目标jobs成功，公开前draft/API/download checksum步骤在log中可见，公开后两个verify jobs成功；publish失败/残留draft时停止，不自动删除tag或覆盖asset。

## 9. 从公开 Release 下载后复核

```powershell
$Out = Join-Path $env:TEMP "mysteries-$Tag-release-check"
if (Test-Path -LiteralPath $Out) { throw "验证目录已存在，请人工确认后清理: $Out" }
New-Item -ItemType Directory -Path $Out | Out-Null
$PublicBase = "https://github.com/$Repo/releases/download/$Tag"
Invoke-WebRequest -Uri "$PublicBase/$($ExpectedAssets[0])" -OutFile (Join-Path $Out $ExpectedAssets[0])
Invoke-WebRequest -Uri "$PublicBase/SHA256SUMS" -OutFile (Join-Path $Out 'SHA256SUMS')

$Zip = Join-Path $Out $ExpectedAssets[0]
$ChecksumLines = Get-Content -LiteralPath (Join-Path $Out 'SHA256SUMS')
$ExpectedLine = @($ChecksumLines | Where-Object { $_ -match "  $([regex]::Escape($ExpectedAssets[0]))$" })
if ($ExpectedLine.Count -ne 1) { throw 'Windows checksum记录缺失或重复' }
$ActualHash = (Get-FileHash -LiteralPath $Zip -Algorithm SHA256).Hash.ToLowerInvariant()
if ($ActualHash -ne ($ExpectedLine[0] -split '\s+')[0].ToLowerInvariant()) { throw 'Windows ZIP checksum不匹配' }

$Expanded = Join-Path $Out 'windows'
Expand-Archive -LiteralPath $Zip -DestinationPath $Expanded
$Files = @(Get-ChildItem -LiteralPath $Expanded -File | Select-Object -ExpandProperty Name | Sort-Object)
if (Compare-Object @('LICENSE','README.md','mysteries.exe') $Files) { throw 'Windows archive文件集异常' }
& (Join-Path $Expanded 'mysteries.exe') --version
& (Join-Path $Expanded 'mysteries.exe') --help
```

随后在Windows Terminal运行下载的`mysteries.exe`：进入TUI、确认header显示v1.2.0、按既有退出流程离开，PowerShell立即可输入。不得覆盖用户现有config/session/credential；使用既有正常配置或隔离测试配置。

Linux公开资产由tag workflow的`Verify published release (linux-x86_64)` job证明公开下载、checksum、文件集、executable bit、`--version`与`--help`；有Linux/WSL环境时可重复：

```bash
curl -fL --retry 3 -O 'https://github.com/tajiaoyezi/mysteries/releases/download/v1.2.0/mysteries-v1.2.0-x86_64-unknown-linux-gnu.tar.gz'
curl -fL --retry 3 -O 'https://github.com/tajiaoyezi/mysteries/releases/download/v1.2.0/SHA256SUMS'
grep 'mysteries-v1.2.0-x86_64-unknown-linux-gnu.tar.gz$' SHA256SUMS | sha256sum --check
tar -xzf mysteries-v1.2.0-x86_64-unknown-linux-gnu.tar.gz
max_glibc="$(readelf --version-info ./mysteries | grep -o 'GLIBC_[0-9.]*' | sort -V | tail -n 1)"
test -n "$max_glibc"
test "$(printf '%s\n' "$max_glibc" 'GLIBC_2.35' | sort -V | tail -n 1)" = 'GLIBC_2.35'
./mysteries --version
./mysteries --help
```

## 10. Archive 决策记录必须包含的真实证据

archive前重新查询并让用户审阅：

- implementation PR number/head/merge SHA与merge时间；
- 精确merge SHA的master CI/Security run IDs、attempts、三个目标job IDs/名称/conclusions/revision markers；
- post-merge release `workflow_dispatch` dry-run ID/attempt/head SHA及jobs，publish skipped；
- annotated tag object SHA、peeled commit、远端ref；
- tag release run ID/attempt/head SHA与全部job IDs/名称/conclusions/`RELEASE_REVISION` markers；
- GitHub Release URL、publishedAt、draft/prerelease/latest状态；
- 三个asset名称/size/API digest（若GitHub提供）及`SHA256SUMS`实际内容；
- Windows/Linux公开下载验证与Windows Terminal真机结果；
- `openspec validate release-v1-2-0 --strict`、`openspec validate --all --strict`、tasks完成数；
- 明确1.0/1.1未补tag、v1.2 binary未提交Git、existing CI/Security未改。

真实证据只写用户批准的archive决策记录，并与spec sync、tasks最终勾选和change move放入同一archive commit。不得再创建递归evidence commit证明archive自身。
