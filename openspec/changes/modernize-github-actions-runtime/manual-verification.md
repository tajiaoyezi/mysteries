# `modernize-github-actions-runtime` 远端证据手册

> 本文件是远端运行证据的持久载体，`tasks.md` 仍是唯一进度状态源。
> implementation evidence 由独立 post-merge evidence commit/PR 提交；该 commit 只证明更早且不可变的 revision，不证明自身。
> 最新 evidence carrier PR 合入后的最终 `master` checks 在 archive 阶段查询，并写入经用户审阅的 archive 决策记录；不得为记录该结果再创建递归 evidence commit。

## 1. 证据语义

`pull_request` 默认执行 `refs/pull/<number>/merge`，不是孤立 PR head；Actions REST `run.head_sha` 在本仓 PR runs 中是 PR head，也不是 runner 的 synthetic merge `GITHUB_SHA`。任何 PR 绿灯记录 MUST 同时保存：

- `PR_HEAD_SHA`：`pull_request.head.sha`；
- `PR_BASE_SHA`：产生该 merge-ref 时的 `pull_request.base.sha`；
- `PR_API_MERGE_SHA`：证据收集时 PR API 的 `merge_commit_sha`；PR 合入后 GitHub 会把 live 字段改为实际 implementation merge commit，事后重放 MUST 使用这里持久记录的值；
- `RUN_HEAD_SHA`：Actions REST workflow run 的 `head_sha`，MUST 等于 `PR_HEAD_SHA`；
- `TESTED_MERGE_SHA`：三个 `Show tested revision` steps 输出的 `TESTED_REVISION=<40hex>` marker，MUST 各恰好出现一次、相等且等于 `PR_API_MERGE_SHA`；
- `TESTED_PARENT_1` / `TESTED_PARENT_2`：merge commit object 的 parents，MUST 分别等于 base/head；
- `RUN_ID` 与 `RUN_ATTEMPT`；
- job conclusion 与相关日志证据。

若 current head/base 与 tested merge parents 不一致，旧 run 只证明旧二元组，不能作为新 revision 的证据。

## 2. 迁移前基线

规划时已确认：

```text
BASELINE_MASTER_SHA=7e2b76950bcd9f9deb1c27bf291d1b9caa6f05f3
BASELINE_CI_RUN_ID=29186434941
BASELINE_SECURITY_RUN_ID=29186434936
BASELINE_CI_RUN_ATTEMPT=1
BASELINE_SECURITY_RUN_ATTEMPT=1
BASELINE_RESULT=CI success; Security audit success
BASELINE_JOB_CONCLUSIONS=fmt · clippy · test · build (ubuntu-latest)=success; fmt · clippy · test · build (windows-latest)=success; RustSec dependency audit=success
BASELINE_WARNING_MATCH_COUNTS=CI=10; Security audit=2
BASELINE_WARNINGS=Node.js 20 forced to Node.js 24; DEP0040 punycode; DEP0169 url.parse()
BASELINE_VERIFIED_AT=2026-07-12T10:28:01.5745102Z
```

实施开始时重新查询最新 `master`；若已前进，在此追加新的 implementation baseline，不覆盖上述规划事实：

```text
IMPLEMENTATION_BASELINE_MASTER_SHA=7e2b76950bcd9f9deb1c27bf291d1b9caa6f05f3
IMPLEMENTATION_BASELINE_CI_RUN_ID=29186434941
IMPLEMENTATION_BASELINE_CI_RUN_ATTEMPT=1
IMPLEMENTATION_BASELINE_SECURITY_RUN_ID=29186434936
IMPLEMENTATION_BASELINE_SECURITY_RUN_ATTEMPT=1
IMPLEMENTATION_BASELINE_WARNING_MATCHES=CI=10; Security audit=2
```

不变量：

```text
CI_TRIGGERS=push master; pull_request
CI_CHECKS=fmt · clippy · test · build (windows-latest); fmt · clippy · test · build (ubuntu-latest)
CI_MATRIX=windows-latest; ubuntu-latest
CI_CACHE_PATHS=~/.cargo/registry; ~/.cargo/git; target
CI_CACHE_KEY=${{ runner.os }}-cargo-${{ hashFiles('**/Cargo.lock') }}
CI_COMMANDS=fmt --check; clippy --locked -D warnings; cargo test --locked; cargo build --release --locked
TARGET_REVISION_STEP=Show tested revision; echo "TESTED_REVISION=$(git rev-parse HEAD)"; both workflows
SECURITY_TRIGGERS=push master; pull_request; weekly schedule; workflow_dispatch
SECURITY_CHECK=RustSec dependency audit
SECURITY_POLICY=cargo-audit 0.22.2; isolated install; strict input validation; absolute binary; --deny unsound; fail-closed
```

## 3. 官方 Action 映射

```text
CHECKOUT_TAG=v7.0.0
CHECKOUT_SHA=9c091bb21b7c1c1d1991bb908d89e4e9dddfe3e0
CHECKOUT_RUNTIME=node24
CACHE_TAG=v6.1.0
CACHE_SHA=55cc8345863c7cc4c66a329aec7e433d2d1c52a9
CACHE_RUNTIME=node24
VERIFIED_AT=2026-07-12T10:28:01.5745102Z
```

实现前必须从官方 repositories 重查 tag ref 和 `action.yml`。映射不一致时停止并调查，不得只更新本文件后继续。

## 4. Implementation PR merge-ref 证据

```text
IMPLEMENTATION_PR_NUMBER=7
PR_HEAD_REPO=tajiaoyezi/mysteries
PR_HEAD_SHA=a9271c73d9c87cee634b33d52b787958c39ec2b7
PR_BASE_SHA=7e2b76950bcd9f9deb1c27bf291d1b9caa6f05f3
PR_API_MERGE_SHA=9a73ad359c489c2b38188d853246384210f79261
TESTED_MERGE_SHA=9a73ad359c489c2b38188d853246384210f79261
TESTED_PARENT_1=7e2b76950bcd9f9deb1c27bf291d1b9caa6f05f3
TESTED_PARENT_2=a9271c73d9c87cee634b33d52b787958c39ec2b7
CI_RUN_ID=29189340999
CI_RUN_ATTEMPT=1
CI_RUN_HEAD_SHA=a9271c73d9c87cee634b33d52b787958c39ec2b7
CI_WINDOWS_JOB_ID=86641170447
CI_WINDOWS_JOB_CONCLUSION=success
CI_WINDOWS_TESTED_REVISION=9a73ad359c489c2b38188d853246384210f79261
CI_UBUNTU_JOB_ID=86641170449
CI_UBUNTU_JOB_CONCLUSION=success
CI_UBUNTU_TESTED_REVISION=9a73ad359c489c2b38188d853246384210f79261
CI_CONCLUSION=success
SECURITY_RUN_ID=29189340994
SECURITY_RUN_ATTEMPT=1
SECURITY_RUN_HEAD_SHA=a9271c73d9c87cee634b33d52b787958c39ec2b7
SECURITY_JOB_ID=86641170426
SECURITY_JOB_CONCLUSION=success
SECURITY_TESTED_REVISION=9a73ad359c489c2b38188d853246384210f79261
SECURITY_CONCLUSION=success
RUNTIME_WARNING_MATCHES=0
```

查询模板（PowerShell 7，仓库根执行）：

```powershell
$repo = gh repo view --json nameWithOwner --jq '.nameWithOwner'
$prNumber = <PR_NUMBER>
$recordedHeadSha = '<PR_HEAD_SHA>'
$recordedBaseSha = '<PR_BASE_SHA>'
$recordedPrApiMergeSha = '<PR_API_MERGE_SHA>'
$recordedTestedMergeSha = '<TESTED_MERGE_SHA>'
$pr = gh api "repos/$repo/pulls/$prNumber" | ConvertFrom-Json
if ($pr.head.repo.full_name -ne $repo) { throw 'implementation PR 必须来自同一 repository 分支，不能用 fork PR 证明 cache save' }
foreach ($recordedSha in @($recordedHeadSha, $recordedBaseSha, $recordedPrApiMergeSha, $recordedTestedMergeSha)) {
    if ($recordedSha -notmatch '^[0-9a-f]{40}$') { throw "持久证据不是完整 SHA: $recordedSha" }
}
if ($recordedPrApiMergeSha -ne $recordedTestedMergeSha) { throw '持久记录的 PR API merge SHA 与 tested merge SHA 不一致' }
if ($pr.head.sha -ne $recordedHeadSha -or $pr.base.sha -ne $recordedBaseSha) { throw '当前 PR head/base 与持久证据不一致' }
if ($pr.state -eq 'open') {
    if (-not $pr.merge_commit_sha) { throw 'PR merge_commit_sha 尚未生成，等待 GitHub 计算 mergeability 后重试' }
    if ($pr.merge_commit_sha -ne $recordedPrApiMergeSha) { throw 'open PR 的 live merge_commit_sha 与持久证据不一致' }
} elseif (-not $pr.merged) {
    throw 'PR 已关闭但未合入，不能重放 implementation merge-ref 证据'
}
$headSha = $recordedHeadSha
$baseSha = $recordedBaseSha
$expectedTestedMergeSha = $recordedTestedMergeSha

gh pr checks $prNumber
$ciRun = gh api "repos/$repo/actions/runs/<CI_RUN_ID>" | ConvertFrom-Json
$securityRun = gh api "repos/$repo/actions/runs/<SECURITY_RUN_ID>" | ConvertFrom-Json
if ($ciRun.event -ne 'pull_request' -or $securityRun.event -ne 'pull_request') { throw '证据 run 不是 pull_request event' }
if ($ciRun.head_sha -ne $headSha -or $securityRun.head_sha -ne $headSha) { throw 'run.head_sha 未绑定当前 PR head' }

$ciJobs = @((gh run view <CI_RUN_ID> --attempt $ciRun.run_attempt --json jobs | ConvertFrom-Json).jobs)
$securityJobs = @((gh run view <SECURITY_RUN_ID> --attempt $securityRun.run_attempt --json jobs | ConvertFrom-Json).jobs)
$windowsJob = @($ciJobs | Where-Object { $_.name -eq 'fmt · clippy · test · build (windows-latest)' })
$ubuntuJob = @($ciJobs | Where-Object { $_.name -eq 'fmt · clippy · test · build (ubuntu-latest)' })
$securityJob = @($securityJobs | Where-Object { $_.name -eq 'RustSec dependency audit' })
if ($windowsJob.Count -ne 1 -or $ubuntuJob.Count -ne 1 -or $securityJob.Count -ne 1) {
    throw 'job/check 名称或数量漂移'
}
foreach ($job in @($windowsJob[0], $ubuntuJob[0], $securityJob[0])) {
    if ($job.conclusion -ne 'success') { throw "job 未成功: $($job.name)" }
}

function Get-TestedRevision {
    param(
        [Parameter(Mandatory)] [long] $RunId,
        [Parameter(Mandatory)] [int] $Attempt,
        [Parameter(Mandatory)] [long] $JobId
    )
    $log = @(gh run view $RunId --attempt $Attempt --job $JobId --log)
    if ($LASTEXITCODE -ne 0) { throw "读取 job log 失败: run=$RunId job=$JobId" }
    $revisionMatches = [regex]::Matches(($log -join "`n"), 'TESTED_REVISION=([0-9a-f]{40})')
    if ($revisionMatches.Count -ne 1) {
        throw "TESTED_REVISION marker 数量异常: run=$RunId job=$JobId count=$($revisionMatches.Count)"
    }
    $revisionMatches[0].Groups[1].Value
}

$windowsRevision = Get-TestedRevision <CI_RUN_ID> $ciRun.run_attempt $windowsJob[0].databaseId
$ubuntuRevision = Get-TestedRevision <CI_RUN_ID> $ciRun.run_attempt $ubuntuJob[0].databaseId
$securityRevision = Get-TestedRevision <SECURITY_RUN_ID> $securityRun.run_attempt $securityJob[0].databaseId
$uniqueRevisions = @($windowsRevision, $ubuntuRevision, $securityRevision) | Select-Object -Unique
if ($uniqueRevisions.Count -ne 1) { throw '三个 jobs 测试的 revision 不一致' }
$testedMergeSha = $windowsRevision
if ($testedMergeSha -ne $expectedTestedMergeSha) { throw 'tested revision 不等于持久记录的 merge-ref SHA' }

$mergeObject = gh api "repos/$repo/git/commits/$testedMergeSha" | ConvertFrom-Json
if ($mergeObject.parents.Count -ne 2) { throw 'tested revision 不是双 parent merge commit' }
if ($mergeObject.parents[0].sha -ne $baseSha -or $mergeObject.parents[1].sha -ne $headSha) {
    throw 'tested merge parents 未绑定当前 base/head'
}

[pscustomobject]@{
    CI_WINDOWS_JOB_ID = $windowsJob[0].databaseId
    CI_WINDOWS_JOB_CONCLUSION = $windowsJob[0].conclusion
    CI_WINDOWS_TESTED_REVISION = $windowsRevision
    CI_UBUNTU_JOB_ID = $ubuntuJob[0].databaseId
    CI_UBUNTU_JOB_CONCLUSION = $ubuntuJob[0].conclusion
    CI_UBUNTU_TESTED_REVISION = $ubuntuRevision
    SECURITY_JOB_ID = $securityJob[0].databaseId
    SECURITY_JOB_CONCLUSION = $securityJob[0].conclusion
    SECURITY_TESTED_REVISION = $securityRevision
    TESTED_MERGE_SHA = $testedMergeSha
    TESTED_PARENT_1 = $mergeObject.parents[0].sha
    TESTED_PARENT_2 = $mergeObject.parents[1].sha
} | Format-List
```

通过条件：

- 两个 runs 的 `event` 都是 `pull_request`；
- 两个 REST `run.head_sha` 都等于 `PR_HEAD_SHA`，不得写入 `TESTED_MERGE_SHA`；
- open PR 的 live `merge_commit_sha` 必须等于已记录的 `PR_API_MERGE_SHA`；PR 合入后该 live 字段不再参与比较，模板改用持久记录的 `PR_API_MERGE_SHA` / `TESTED_MERGE_SHA`；
- 三个目标 jobs 的日志各恰好包含一个 `TESTED_REVISION=<40hex>` marker，三个 SHA 相等并等于持久记录的 `TESTED_MERGE_SHA`；
- `TESTED_MERGE_SHA` 的 first/second parents 分别等于记录的 `PR_BASE_SHA` / `PR_HEAD_SHA`；
- 三个 jobs 均为 `success`；
- 完整日志与 annotations 中四类 warning 匹配数均为 0。

日志扫描模板：

```powershell
$patterns = 'Node.js 20|forced to run on Node.js 24|DEP0040|punycode|DEP0169|url\.parse'
function Get-RunWarningEvidence {
    param([long] $RunId, [int] $Attempt)
    $summary = @(gh run view $RunId --attempt $Attempt)
    if ($LASTEXITCODE -ne 0) { throw "读取 run summary/annotations 失败: run=$RunId attempt=$Attempt" }
    $log = @(gh run view $RunId --attempt $Attempt --log)
    if ($LASTEXITCODE -ne 0) { throw "读取 run log 失败: run=$RunId attempt=$Attempt" }
    @($summary) + @($log)
}
$warningEvidence = @()
$warningEvidence += @(Get-RunWarningEvidence <CI_RUN_ID> <CI_RUN_ATTEMPT>)
$warningEvidence += @(Get-RunWarningEvidence <SECURITY_RUN_ID> <SECURITY_RUN_ATTEMPT>)
$warningMatches = @($warningEvidence | Select-String -Pattern $patterns)
if ($warningMatches.Count -ne 0) { $warningMatches; throw 'PR runs 的日志或 annotations 仍有 runtime/cache deprecation' }
```

## 5. Cache 证据

每个平台分别填写：

```text
WINDOWS_CACHE_FIRST_RESULT=hit
WINDOWS_CACHE_SAVE_RESULT=not-needed
WINDOWS_CACHE_FIRST_ATTEMPT=1
WINDOWS_CACHE_RERUN_ATTEMPT=not-needed
WINDOWS_CACHE_RERUN_RESTORE=not-needed
WINDOWS_CACHE_RERUN_TESTED_REVISION=not-needed
UBUNTU_CACHE_FIRST_RESULT=hit
UBUNTU_CACHE_SAVE_RESULT=not-needed
UBUNTU_CACHE_FIRST_ATTEMPT=1
UBUNTU_CACHE_RERUN_ATTEMPT=not-needed
UBUNTU_CACHE_RERUN_RESTORE=not-needed
UBUNTU_CACHE_RERUN_TESTED_REVISION=not-needed
```

本 change 的 save/restore 证据 MUST 使用 §4 已断言为同仓分支的 implementation PR。外部 fork PR 若收到 read-only cache token，其显式 save-denied warning 允许继续成功，不作为本节 save 能力证据，也不得改用 `pull_request_target`。

若首轮 miss，先用首轮 attempt 日志确认 post-job save 成功，再执行：

```powershell
$repo = gh repo view --json nameWithOwner --jq '.nameWithOwner'
$ciRunId = <CI_RUN_ID>
$firstAttempt = <FIRST_ATTEMPT>
$expectedHeadSha = '<PR_HEAD_SHA>'
$expectedTestedMergeSha = '<TESTED_MERGE_SHA>'
$windowsFirstResult = '<hit|miss>'
$ubuntuFirstResult = '<hit|miss>'

$firstJobs = @((gh run view $ciRunId --attempt $firstAttempt --json jobs | ConvertFrom-Json).jobs)
if ($LASTEXITCODE -ne 0) { throw '读取首轮 CI jobs 失败' }
$firstWindowsJob = @($firstJobs | Where-Object { $_.name -eq 'fmt · clippy · test · build (windows-latest)' })
$firstUbuntuJob = @($firstJobs | Where-Object { $_.name -eq 'fmt · clippy · test · build (ubuntu-latest)' })
if ($firstWindowsJob.Count -ne 1 -or $firstUbuntuJob.Count -ne 1) { throw '首轮 CI job/check 名称或数量漂移' }
foreach ($job in @($firstWindowsJob[0], $firstUbuntuJob[0])) {
    if ($job.conclusion -ne 'success') { throw "首轮 CI job 未成功: $($job.name)" }
}

function Assert-FirstCacheResult {
    param(
        [Parameter(Mandatory)] [long] $RunId,
        [Parameter(Mandatory)] [int] $Attempt,
        [Parameter(Mandatory)] [long] $JobId,
        [Parameter(Mandatory)] [ValidateSet('hit', 'miss')] [string] $ExpectedResult
    )
    $log = @(gh run view $RunId --attempt $Attempt --job $JobId --log)
    if ($LASTEXITCODE -ne 0) { throw "读取首轮 cache job log 失败: run=$RunId job=$JobId" }
    $text = $log -join "`n"
    if ($text -match 'Failed to save') { throw "首轮 cache save 失败: run=$RunId job=$JobId" }
    if ($ExpectedResult -eq 'hit') {
        if ($text -notmatch 'Cache restored|Cache hit occurred on the primary key') {
            throw "首轮未证明 cache hit: run=$RunId job=$JobId"
        }
        return [pscustomobject]@{ Result = 'hit'; Save = 'not-needed' }
    }
    if ($text -notmatch 'Cache not found') { throw "首轮未证明 cache miss: run=$RunId job=$JobId" }
    if ($text -notmatch 'Cache saved successfully') { throw "首轮 miss 后未证明 cache save: run=$RunId job=$JobId" }
    [pscustomobject]@{ Result = 'miss'; Save = 'success' }
}

$windowsFirst = Assert-FirstCacheResult $ciRunId $firstAttempt $firstWindowsJob[0].databaseId $windowsFirstResult
$ubuntuFirst = Assert-FirstCacheResult $ciRunId $firstAttempt $firstUbuntuJob[0].databaseId $ubuntuFirstResult
if ($windowsFirst.Result -ne 'miss' -and $ubuntuFirst.Result -ne 'miss') {
    throw '两个平台首轮均 hit：无需执行本 rerun 模板，只记录 restore 证据'
}

gh run rerun $ciRunId
if ($LASTEXITCODE -ne 0) { throw '触发 CI rerun 失败' }
gh run watch $ciRunId --exit-status
if ($LASTEXITCODE -ne 0) { throw 'CI rerun watch 失败或 run 未成功' }
$rerun = gh api "repos/$repo/actions/runs/$ciRunId" | ConvertFrom-Json
if ($LASTEXITCODE -ne 0) { throw '读取 CI rerun API 失败' }
$rerun | Select-Object id,run_attempt,head_sha,status,conclusion
if ($rerun.run_attempt -le $firstAttempt) { throw 'rerun attempt 未递增' }
if ($rerun.head_sha -ne $expectedHeadSha -or $rerun.conclusion -ne 'success') { throw 'rerun 未绑定原 PR head 或未成功' }

$rerunJobs = @((gh run view $ciRunId --attempt $rerun.run_attempt --json jobs | ConvertFrom-Json).jobs)
$rerunWindowsJob = @($rerunJobs | Where-Object { $_.name -eq 'fmt · clippy · test · build (windows-latest)' })
$rerunUbuntuJob = @($rerunJobs | Where-Object { $_.name -eq 'fmt · clippy · test · build (ubuntu-latest)' })
if ($rerunWindowsJob.Count -ne 1 -or $rerunUbuntuJob.Count -ne 1) { throw 'rerun CI job/check 名称或数量漂移' }
foreach ($job in @($rerunWindowsJob[0], $rerunUbuntuJob[0])) {
    if ($job.conclusion -ne 'success') { throw "rerun job 未成功: $($job.name)" }
}

function Get-RerunEvidence {
    param(
        [Parameter(Mandatory)] [long] $RunId,
        [Parameter(Mandatory)] [int] $Attempt,
        [Parameter(Mandatory)] [long] $JobId
    )
    $log = @(gh run view $RunId --attempt $Attempt --job $JobId --log)
    if ($LASTEXITCODE -ne 0) { throw "读取 rerun job log 失败: run=$RunId job=$JobId" }
    $revisionMatches = [regex]::Matches(($log -join "`n"), 'TESTED_REVISION=([0-9a-f]{40})')
    if ($revisionMatches.Count -ne 1) {
        throw "rerun TESTED_REVISION marker 数量异常: run=$RunId job=$JobId count=$($revisionMatches.Count)"
    }
    if (($log -join "`n") -notmatch 'Cache restored|Cache hit occurred on the primary key') {
        throw "rerun 未证明 cache restore: run=$RunId job=$JobId"
    }
    [pscustomobject]@{ Revision = $revisionMatches[0].Groups[1].Value; Log = $log }
}

$windowsRerun = Get-RerunEvidence $ciRunId $rerun.run_attempt $rerunWindowsJob[0].databaseId
$ubuntuRerun = Get-RerunEvidence $ciRunId $rerun.run_attempt $rerunUbuntuJob[0].databaseId
if ($windowsRerun.Revision -ne $expectedTestedMergeSha -or $ubuntuRerun.Revision -ne $expectedTestedMergeSha) {
    throw 'rerun tested revision 不等于首轮 TESTED_MERGE_SHA'
}
[pscustomobject]@{
    WINDOWS_CACHE_FIRST_RESULT = $windowsFirst.Result
    WINDOWS_CACHE_SAVE_RESULT = $windowsFirst.Save
    WINDOWS_CACHE_FIRST_ATTEMPT = $firstAttempt
    WINDOWS_CACHE_RERUN_ATTEMPT = $rerun.run_attempt
    WINDOWS_CACHE_RERUN_RESTORE = 'success'
    WINDOWS_CACHE_RERUN_TESTED_REVISION = $windowsRerun.Revision
    UBUNTU_CACHE_FIRST_RESULT = $ubuntuFirst.Result
    UBUNTU_CACHE_SAVE_RESULT = $ubuntuFirst.Save
    UBUNTU_CACHE_FIRST_ATTEMPT = $firstAttempt
    UBUNTU_CACHE_RERUN_ATTEMPT = $rerun.run_attempt
    UBUNTU_CACHE_RERUN_RESTORE = 'success'
    UBUNTU_CACHE_RERUN_TESTED_REVISION = $ubuntuRerun.Revision
} | Format-List
```

脚本 MUST 从 rerun attempt 的两个目标 CI jobs 各提取恰好一个 marker，并断言两者仍等于首轮 `TESTED_MERGE_SHA`；REST `head_sha` 仍等于 `PR_HEAD_SHA`，只允许 `run_attempt` 增加。首轮已 hit 时保存 restore 日志即可，不清空 cache 制造 miss。

## 6. Implementation merge 的 master 证据

```text
IMPLEMENTATION_MERGE_SHA=b3a6c52401543903251d76c31ae9d73468cb4f89
MASTER_CI_RUN_ID=29189491734
MASTER_CI_RUN_ATTEMPT=1
MASTER_CI_RUN_HEAD_SHA=b3a6c52401543903251d76c31ae9d73468cb4f89
MASTER_CI_CONCLUSION=success
MASTER_CI_WINDOWS_JOB_ID=86641560557
MASTER_CI_WINDOWS_JOB_CONCLUSION=success
MASTER_CI_WINDOWS_TESTED_REVISION=b3a6c52401543903251d76c31ae9d73468cb4f89
MASTER_CI_UBUNTU_JOB_ID=86641560564
MASTER_CI_UBUNTU_JOB_CONCLUSION=success
MASTER_CI_UBUNTU_TESTED_REVISION=b3a6c52401543903251d76c31ae9d73468cb4f89
MASTER_SECURITY_RUN_ID=29189491706
MASTER_SECURITY_RUN_ATTEMPT=1
MASTER_SECURITY_RUN_HEAD_SHA=b3a6c52401543903251d76c31ae9d73468cb4f89
MASTER_SECURITY_CONCLUSION=success
MASTER_SECURITY_JOB_ID=86641560541
MASTER_SECURITY_JOB_CONCLUSION=success
MASTER_SECURITY_TESTED_REVISION=b3a6c52401543903251d76c31ae9d73468cb4f89
MASTER_RUNTIME_WARNING_MATCHES=0
```

查询模板：

```powershell
$repo = gh repo view --json nameWithOwner --jq '.nameWithOwner'
$implementationPr = gh pr view <IMPLEMENTATION_PR_NUMBER> --json mergeCommit,state | ConvertFrom-Json
$implementationMergeSha = $implementationPr.mergeCommit.oid
if ($implementationPr.state -ne 'MERGED') { throw 'implementation PR 尚未合入' }

$masterCiRuns = @(gh run list --workflow ci.yml --branch master --commit $implementationMergeSha --event push --limit 10 --json databaseId,name,event,status,conclusion,headSha,url | ConvertFrom-Json)
$masterSecurityRuns = @(gh run list --workflow security-audit.yml --branch master --commit $implementationMergeSha --event push --limit 10 --json databaseId,name,event,status,conclusion,headSha,url | ConvertFrom-Json)
if ($masterCiRuns.Count -ne 1 -or $masterSecurityRuns.Count -ne 1) { throw '无法唯一定位 implementation merge 的 push runs' }
if ($masterCiRuns[0].event -ne 'push' -or $masterCiRuns[0].conclusion -ne 'success' -or $masterCiRuns[0].headSha -ne $implementationMergeSha) { throw 'implementation merge push CI 未通过' }
if ($masterSecurityRuns[0].event -ne 'push' -or $masterSecurityRuns[0].conclusion -ne 'success' -or $masterSecurityRuns[0].headSha -ne $implementationMergeSha) { throw 'implementation merge push Security audit 未通过' }

$masterCiRunApi = gh api "repos/$repo/actions/runs/$($masterCiRuns[0].databaseId)" | ConvertFrom-Json
$masterSecurityRunApi = gh api "repos/$repo/actions/runs/$($masterSecurityRuns[0].databaseId)" | ConvertFrom-Json
$masterCiJobs = @((gh run view $masterCiRuns[0].databaseId --attempt $masterCiRunApi.run_attempt --json jobs | ConvertFrom-Json).jobs)
$masterSecurityJobs = @((gh run view $masterSecurityRuns[0].databaseId --attempt $masterSecurityRunApi.run_attempt --json jobs | ConvertFrom-Json).jobs)
$masterWindowsJob = @($masterCiJobs | Where-Object { $_.name -eq 'fmt · clippy · test · build (windows-latest)' })
$masterUbuntuJob = @($masterCiJobs | Where-Object { $_.name -eq 'fmt · clippy · test · build (ubuntu-latest)' })
$masterSecurityJob = @($masterSecurityJobs | Where-Object { $_.name -eq 'RustSec dependency audit' })
if ($masterWindowsJob.Count -ne 1 -or $masterUbuntuJob.Count -ne 1 -or $masterSecurityJob.Count -ne 1) {
    throw 'implementation merge job/check 名称或数量漂移'
}
foreach ($job in @($masterWindowsJob[0], $masterUbuntuJob[0], $masterSecurityJob[0])) {
    if ($job.conclusion -ne 'success') { throw "implementation merge job 未成功: $($job.name)" }
}

function Get-MasterTestedRevision {
    param([long] $RunId, [int] $Attempt, [long] $JobId)
    $log = @(gh run view $RunId --attempt $Attempt --job $JobId --log)
    if ($LASTEXITCODE -ne 0) { throw "读取 master job log 失败: run=$RunId job=$JobId" }
    $matches = [regex]::Matches(($log -join "`n"), 'TESTED_REVISION=([0-9a-f]{40})')
    if ($matches.Count -ne 1) { throw "master TESTED_REVISION marker 数量异常: run=$RunId job=$JobId count=$($matches.Count)" }
    $matches[0].Groups[1].Value
}

$masterWindowsRevision = Get-MasterTestedRevision $masterCiRuns[0].databaseId $masterCiRunApi.run_attempt $masterWindowsJob[0].databaseId
$masterUbuntuRevision = Get-MasterTestedRevision $masterCiRuns[0].databaseId $masterCiRunApi.run_attempt $masterUbuntuJob[0].databaseId
$masterSecurityRevision = Get-MasterTestedRevision $masterSecurityRuns[0].databaseId $masterSecurityRunApi.run_attempt $masterSecurityJob[0].databaseId
if ($masterWindowsRevision -ne $implementationMergeSha -or $masterUbuntuRevision -ne $implementationMergeSha -or $masterSecurityRevision -ne $implementationMergeSha) {
    throw 'implementation merge 三个 jobs 未测试 IMPLEMENTATION_MERGE_SHA'
}

$patterns = 'Node.js 20|forced to run on Node.js 24|DEP0040|punycode|DEP0169|url\.parse'
function Get-MasterWarningEvidence {
    param([long] $RunId, [int] $Attempt)
    $summary = @(gh run view $RunId --attempt $Attempt)
    if ($LASTEXITCODE -ne 0) { throw "读取 master run summary/annotations 失败: run=$RunId attempt=$Attempt" }
    $log = @(gh run view $RunId --attempt $Attempt --log)
    if ($LASTEXITCODE -ne 0) { throw "读取 master run log 失败: run=$RunId attempt=$Attempt" }
    @($summary) + @($log)
}
$warningEvidence = @()
$warningEvidence += @(Get-MasterWarningEvidence $masterCiRuns[0].databaseId $masterCiRunApi.run_attempt)
$warningEvidence += @(Get-MasterWarningEvidence $masterSecurityRuns[0].databaseId $masterSecurityRunApi.run_attempt)
$warningMatches = @($warningEvidence | Select-String -Pattern $patterns)
if ($warningMatches.Count -ne 0) { $warningMatches; throw 'implementation merge 的日志或 annotations 仍有 runtime/cache deprecation' }
```

两个 push runs 的 `head_sha` 与三个 marker MUST 精确等于 `IMPLEMENTATION_MERGE_SHA`；三个目标 jobs 必须按精确名称唯一定位且均为 `success`，四类 warning 匹配数须为 0。run attempts、job IDs、conclusions、markers 与日志 MUST 持久记录。

## 7. Post-merge evidence 与 archive gate

post-merge evidence branch 创建后，先把其 branch name 填入本文件，再以一个原子 evidence commit 提交第 2–6 节与 `tasks.md` 5.1–5.4 完成状态。5.4 checkbox 的完成边界仅为该 branch 与 durable evidence commit 已创建；不得要求 commit 在自身内容里记录尚未存在的 SHA/PR number。

若 evidence PR 合入后的只读审查发现本手册或其他 OpenSpec artifact 有缺陷，MAY 通过新的 bounded review-remediation evidence carrier 修复，但 MUST 同时满足：实现侧不得修改 workflow、Rust、Cargo 或其他产品文件；新 branch name 必须在其 commit 前写入 `EVIDENCE_BRANCH`；原 carrier 写入 `SUPERSEDED_EVIDENCE_BRANCH`；commit 不得记录自身尚未存在的 SHA/PR number；新 PR 合入后必须用本节 archive gate 验证其精确 merge SHA。任何后续 remediation 都必须重复该 carrier 轮换，不得直接把未提交修改静默带入 archive。

当前最新 carrier：

```text
EVIDENCE_BRANCH=codex/modernize-github-actions-runtime-review-fix
SUPERSEDED_EVIDENCE_BRANCH=codex/modernize-github-actions-runtime-evidence
```

最新 evidence carrier commit 的 push、PR 创建/合入及其最终 `master` checks 是非 checkbox archive precondition。carrier PR 合入后不得直接回写本文件；若需 review remediation，必须先按上述规则轮换 carrier。archive 前执行：

```powershell
$repo = gh repo view --json nameWithOwner --jq '.nameWithOwner'
$evidenceBranch = '<EVIDENCE_BRANCH>'
$evidencePrs = @(gh pr list --state merged --head $evidenceBranch --json number,headRefOid,mergeCommit,state,url | ConvertFrom-Json)
if ($evidencePrs.Count -ne 1) { throw "无法唯一定位 evidence PR: $evidenceBranch" }
$evidencePr = $evidencePrs[0]
$evidenceMergeSha = $evidencePr.mergeCommit.oid
if ($evidencePr.state -ne 'MERGED') { throw 'evidence PR 尚未合入' }

$ciRuns = @(gh run list --workflow ci.yml --branch master --commit $evidenceMergeSha --event push --limit 10 --json databaseId,name,event,status,conclusion,headSha,url | ConvertFrom-Json)
$securityRuns = @(gh run list --workflow security-audit.yml --branch master --commit $evidenceMergeSha --event push --limit 10 --json databaseId,name,event,status,conclusion,headSha,url | ConvertFrom-Json)
if ($ciRuns.Count -ne 1) { throw '无法唯一定位 evidence merge 的 push CI run' }
if ($securityRuns.Count -ne 1) { throw '无法唯一定位 evidence merge 的 push Security audit run' }
$runs = @($ciRuns[0], $securityRuns[0])
$runs | Format-Table name,databaseId,event,status,conclusion,headSha
if ($ciRuns[0].event -ne 'push' -or $ciRuns[0].conclusion -ne 'success' -or $ciRuns[0].headSha -ne $evidenceMergeSha) { throw 'evidence merge push CI 未通过' }
if ($securityRuns[0].event -ne 'push' -or $securityRuns[0].conclusion -ne 'success' -or $securityRuns[0].headSha -ne $evidenceMergeSha) { throw 'evidence merge push Security audit 未通过' }

$ciRunApi = gh api "repos/$repo/actions/runs/$($ciRuns[0].databaseId)" | ConvertFrom-Json
$securityRunApi = gh api "repos/$repo/actions/runs/$($securityRuns[0].databaseId)" | ConvertFrom-Json
$ciRunApi | Select-Object id,run_attempt,head_sha,event,status,conclusion,html_url
$securityRunApi | Select-Object id,run_attempt,head_sha,event,status,conclusion,html_url

$ciJobs = @((gh run view $ciRuns[0].databaseId --attempt $ciRunApi.run_attempt --json jobs | ConvertFrom-Json).jobs)
$securityJobs = @((gh run view $securityRuns[0].databaseId --attempt $securityRunApi.run_attempt --json jobs | ConvertFrom-Json).jobs)
$windowsJob = @($ciJobs | Where-Object { $_.name -eq 'fmt · clippy · test · build (windows-latest)' })
$ubuntuJob = @($ciJobs | Where-Object { $_.name -eq 'fmt · clippy · test · build (ubuntu-latest)' })
$securityJob = @($securityJobs | Where-Object { $_.name -eq 'RustSec dependency audit' })
if ($windowsJob.Count -ne 1 -or $ubuntuJob.Count -ne 1 -or $securityJob.Count -ne 1) {
    throw 'evidence merge job/check 名称或数量漂移'
}
foreach ($job in @($windowsJob[0], $ubuntuJob[0], $securityJob[0])) {
    if ($job.conclusion -ne 'success') { throw "evidence merge job 未成功: $($job.name)" }
}

function Get-ArchiveTestedRevision {
    param([long] $RunId, [int] $Attempt, [long] $JobId)
    $log = @(gh run view $RunId --attempt $Attempt --job $JobId --log)
    if ($LASTEXITCODE -ne 0) { throw "读取 archive gate job log 失败: run=$RunId job=$JobId" }
    $matches = [regex]::Matches(($log -join "`n"), 'TESTED_REVISION=([0-9a-f]{40})')
    if ($matches.Count -ne 1) { throw "archive gate TESTED_REVISION marker 数量异常: run=$RunId job=$JobId count=$($matches.Count)" }
    $matches[0].Groups[1].Value
}

$windowsRevision = Get-ArchiveTestedRevision $ciRuns[0].databaseId $ciRunApi.run_attempt $windowsJob[0].databaseId
$ubuntuRevision = Get-ArchiveTestedRevision $ciRuns[0].databaseId $ciRunApi.run_attempt $ubuntuJob[0].databaseId
$securityRevision = Get-ArchiveTestedRevision $securityRuns[0].databaseId $securityRunApi.run_attempt $securityJob[0].databaseId
if ($windowsRevision -ne $evidenceMergeSha -or $ubuntuRevision -ne $evidenceMergeSha -or $securityRevision -ne $evidenceMergeSha) {
    throw 'evidence merge 三个 jobs 未测试 evidence merge SHA'
}

$patterns = 'Node.js 20|forced to run on Node.js 24|DEP0040|punycode|DEP0169|url\.parse'
function Get-ArchiveWarningEvidence {
    param([long] $RunId, [int] $Attempt)
    $summary = @(gh run view $RunId --attempt $Attempt)
    if ($LASTEXITCODE -ne 0) { throw "读取 archive run summary/annotations 失败: run=$RunId attempt=$Attempt" }
    $log = @(gh run view $RunId --attempt $Attempt --log)
    if ($LASTEXITCODE -ne 0) { throw "读取 archive run log 失败: run=$RunId attempt=$Attempt" }
    @($summary) + @($log)
}
$warningEvidence = @()
$warningEvidence += @(Get-ArchiveWarningEvidence $ciRuns[0].databaseId $ciRunApi.run_attempt)
$warningEvidence += @(Get-ArchiveWarningEvidence $securityRuns[0].databaseId $securityRunApi.run_attempt)
$warningMatches = @($warningEvidence | Select-String -Pattern $patterns)
if ($warningMatches.Count -ne 0) { $warningMatches; throw 'evidence merge 的日志或 annotations 仍有 runtime/cache deprecation' }
```

随后把以下内容写入用户审阅的 archive 决策记录：

- evidence merge SHA；
- evidence PR number、branch 与 headRefOid；
- CI/Security run IDs 与 attempts；
- 三个 jobs 的 IDs、精确名称、结论与 `TESTED_REVISION` markers；
- runtime/cache warning 扫描结论；
- `openspec validate --all --strict` 结果。

## 8. 最终通过标准

- PR evidence 固定 head/base/API merge/tested merge-ref及其 parents，REST run head 绑定 PR head，三个 jobs 确实测试同一 merge-ref SHA；
- PR Windows/Ubuntu/Security 三个 jobs 全绿且四类 warning 为 0；
- cache hit 有 restore 证据；miss 有 save + 同 merge-ref rerun restore 证据；
- implementation merge SHA 的 master CI/Security 三个目标 jobs 全绿、markers 等于该 merge SHA 且 warning 为 0；
- post-merge evidence 已持久提交；
- evidence merge SHA 的最终 master gates 已按精确 job 名称、结论与 markers 查询，并写入用户批准的决策记录。
