[CmdletBinding()]
param(
    [ValidateSet('Run', 'Smoke', 'CleanupStale')]
    [string]$Action = 'Run',

    [ValidateSet('All', '9.3', '9.4', '9.5')]
    [string]$Section = 'All'
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$PointerName = 'mysteries-readonly-subagent-manual.current'
$TombstoneName = 'mysteries-readonly-subagent-manual.cleanup.json'
$RootPrefix = 'mysteries-readonly-subagent-manual-'
$TerminalJobStates = @('Completed', 'Failed', 'Stopped')
$OracleResults = [System.Collections.Generic.List[object]]::new()

function Write-Phase {
    param([Parameter(Mandatory)][string]$Title)

    Write-Host ''
    Write-Host ('=' * 72) -ForegroundColor DarkCyan
    Write-Host $Title -ForegroundColor Cyan
    Write-Host ('=' * 72) -ForegroundColor DarkCyan
}

function New-AtomicTextFile {
    param(
        [Parameter(Mandatory)][string]$Path,
        [Parameter(Mandatory)][string]$Content
    )

    $Bytes = [System.Text.UTF8Encoding]::new($false).GetBytes($Content)
    $Stream = [System.IO.File]::Open(
        $Path,
        [System.IO.FileMode]::CreateNew,
        [System.IO.FileAccess]::Write,
        [System.IO.FileShare]::None
    )
    try {
        $Stream.Write($Bytes, 0, $Bytes.Length)
    } finally {
        $Stream.Dispose()
    }
}

function Assert-NotReparsePoint {
    param(
        [Parameter(Mandatory)][string]$Path,
        [Parameter(Mandatory)][string]$Label
    )

    $Item = Get-Item -Force -LiteralPath $Path
    if (($Item.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -ne 0) {
        throw "$Label 不得为reparse point: $Path"
    }
}

function Get-FixturePointerInfo {
    param([Parameter(Mandatory)][string]$PointerPath)

    if (-not (Test-Path -LiteralPath $PointerPath -PathType Leaf)) {
        throw "fixture指针不存在: $PointerPath"
    }
    Assert-NotReparsePoint -Path $PointerPath -Label 'fixture pointer'

    $ResolvedTemp = [System.IO.Path]::GetFullPath($env:TEMP)
    $ResolvedRoot = [System.IO.Path]::GetFullPath(
        (Get-Content -Raw -LiteralPath $PointerPath).Trim()
    )

    $ParentIsTemp = [System.IO.Path]::GetDirectoryName($ResolvedRoot).Equals(
        $ResolvedTemp,
        [System.StringComparison]::OrdinalIgnoreCase
    )
    $RootName = [System.IO.Path]::GetFileName($ResolvedRoot)
    if (-not $ParentIsTemp -or -not $RootName.StartsWith(
        $RootPrefix,
        [System.StringComparison]::Ordinal
    )) {
        throw "fixture root不在允许的TEMP边界: $ResolvedRoot"
    }

    $RunId = $RootName.Substring($RootPrefix.Length)
    if ($RunId -cnotmatch '^[0-9a-f]{32}$') {
        throw "fixture run id不是32位小写hex GUID N: $ResolvedRoot"
    }

    [pscustomobject]@{
        Root = $ResolvedRoot
        RunId = $RunId
        RootExists = Test-Path -LiteralPath $ResolvedRoot -PathType Container
    }
}

function Get-ValidatedPorts {
    param(
        [Parameter(Mandatory)]$State,
        [Parameter(Mandatory)][string]$Label
    )

    if ($null -eq $State.PSObject.Properties['ports']) {
        throw "$Label 缺少ports"
    }
    $Ports = @($State.ports)
    if ($Ports.Count -ne 4 -or
        @($Ports | Where-Object {
            $_ -isnot [long] -and $_ -isnot [int]
        }).Count -gt 0 -or
        @($Ports | Where-Object {
            [int]$_ -lt 1 -or [int]$_ -gt 65535
        }).Count -gt 0 -or
        @($Ports | Select-Object -Unique).Count -ne 4) {
        throw "$Label ports无效"
    }
    return @($Ports | ForEach-Object { [int]$_ })
}

function Assert-OwnerIdentityShape {
    param(
        [Parameter(Mandatory)]$State,
        [Parameter(Mandatory)][string]$Label
    )

    $PidProperty = $State.PSObject.Properties['owner_pid']
    $TicksProperty = $State.PSObject.Properties['owner_start_utc_ticks']
    if ($null -eq $PidProperty -or
        $null -eq $TicksProperty -or
        ($PidProperty.Value -isnot [int] -and
            $PidProperty.Value -isnot [long]) -or
        ($TicksProperty.Value -isnot [int] -and
            $TicksProperty.Value -isnot [long]) -or
        [long]$PidProperty.Value -le 0 -or
        [long]$TicksProperty.Value -le 0) {
        throw "$Label owner identity无效"
    }
}

function Get-OwnedFixture {
    param([Parameter(Mandatory)][string]$PointerPath)

    $Pointer = Get-FixturePointerInfo -PointerPath $PointerPath
    if (-not $Pointer.RootExists) {
        throw "fixture root不存在: $($Pointer.Root)"
    }
    Assert-NotReparsePoint -Path $Pointer.Root -Label 'fixture root'

    $ResolvedRoot = $Pointer.Root
    $RunId = $Pointer.RunId
    $OwnerSentinel = Join-Path $ResolvedRoot '.fixture-owner'
    $StatePath = Join-Path $ResolvedRoot 'state.json'
    if (-not (Test-Path -LiteralPath $OwnerSentinel -PathType Leaf) -or
        -not (Test-Path -LiteralPath $StatePath -PathType Leaf)) {
        throw "fixture ownership文件缺失: $ResolvedRoot"
    }
    Assert-NotReparsePoint -Path $OwnerSentinel -Label 'owner sentinel'
    Assert-NotReparsePoint -Path $StatePath -Label 'state file'

    $OwnerToken = (Get-Content -Raw -LiteralPath $OwnerSentinel).Trim()
    $State = Get-Content -Raw -LiteralPath $StatePath | ConvertFrom-Json
    if ($null -eq $State.PSObject.Properties['run_id']) {
        throw "fixture state缺少run_id: $StatePath"
    }
    if ($OwnerToken -cne $RunId -or [string]$State.run_id -cne $RunId) {
        throw "fixture ownership token不匹配: $ResolvedRoot"
    }
    [void](Get-ValidatedPorts -State $State -Label "fixture state: $StatePath")
    Assert-OwnerIdentityShape -State $State -Label "fixture state: $StatePath"

    [pscustomobject]@{
        Root = $ResolvedRoot
        RunId = $RunId
        OwnerSentinel = $OwnerSentinel
        StatePath = $StatePath
        State = $State
    }
}

function Get-CleanupTombstone {
    param([Parameter(Mandatory)][string]$TombstonePath)

    if (-not (Test-Path -LiteralPath $TombstonePath -PathType Leaf)) {
        throw "cleanup tombstone不存在: $TombstonePath"
    }
    Assert-NotReparsePoint -Path $TombstonePath -Label 'cleanup tombstone'
    $Manifest = Get-Content -Raw -LiteralPath $TombstonePath | ConvertFrom-Json
    if ($null -eq $Manifest.PSObject.Properties['version'] -or
        [long]$Manifest.version -ne 1) {
        throw "cleanup tombstone version无效: $TombstonePath"
    }

    if ($null -eq $Manifest.PSObject.Properties['run_id'] -or
        $null -eq $Manifest.PSObject.Properties['root']) {
        throw "cleanup tombstone缺少root/run_id: $TombstonePath"
    }
    $RunId = [string]$Manifest.run_id
    if ($RunId -cnotmatch '^[0-9a-f]{32}$') {
        throw "cleanup tombstone run id无效: $TombstonePath"
    }
    $ResolvedRoot = [System.IO.Path]::GetFullPath([string]$Manifest.root)
    $ResolvedTemp = [System.IO.Path]::GetFullPath($env:TEMP)
    $ExpectedName = "$RootPrefix$RunId"
    $RootIsBounded =
        [System.IO.Path]::GetDirectoryName($ResolvedRoot).Equals(
            $ResolvedTemp,
            [System.StringComparison]::OrdinalIgnoreCase
        ) -and
        [System.IO.Path]::GetFileName($ResolvedRoot).Equals(
            $ExpectedName,
            [System.StringComparison]::Ordinal
        )
    if (-not $RootIsBounded) {
        throw "cleanup tombstone root不在严格TEMP/GUID边界: $ResolvedRoot"
    }

    $Ports = @(Get-ValidatedPorts `
        -State $Manifest `
        -Label "cleanup tombstone: $TombstonePath")
    Assert-OwnerIdentityShape `
        -State $Manifest `
        -Label "cleanup tombstone: $TombstonePath"

    if (Test-Path -LiteralPath $ResolvedRoot) {
        if (-not (Test-Path -LiteralPath $ResolvedRoot -PathType Container)) {
            throw "cleanup tombstone root不是目录: $ResolvedRoot"
        }
        Assert-NotReparsePoint -Path $ResolvedRoot -Label 'tombstone fixture root'
    }

    [pscustomobject]@{
        Root = $ResolvedRoot
        RunId = $RunId
        Ports = $Ports
        OwnerState = $Manifest
    }
}

function Assert-PointerMatches {
    param(
        [Parameter(Mandatory)][string]$PointerPath,
        [Parameter(Mandatory)][string]$Root,
        [Parameter(Mandatory)][string]$RunId
    )

    if (-not (Test-Path -LiteralPath $PointerPath)) {
        return
    }
    $Pointer = Get-FixturePointerInfo -PointerPath $PointerPath
    if (-not $Pointer.Root.Equals(
        $Root,
        [System.StringComparison]::OrdinalIgnoreCase
    ) -or $Pointer.RunId -cne $RunId) {
        throw 'current pointer与待清理fixture不匹配，拒绝删除'
    }
}

function Test-TcpPortClosed {
    param([Parameter(Mandatory)][int]$Port)

    $Probe = [System.Net.Sockets.TcpListener]::new(
        [System.Net.IPAddress]::Loopback,
        $Port
    )
    try {
        $Probe.Server.ExclusiveAddressUse = $true
        try {
            $Probe.Start()
            return $true
        } catch [System.Net.Sockets.SocketException] {
            return $false
        }
    } finally {
        try {
            $Probe.Stop()
        } catch {
            # 未成功bind时Stop本身也不得掩盖原判定。
        }
    }
}

function Wait-FixturePortsClosed {
    param(
        [Parameter(Mandatory)][object[]]$Ports,
        [int]$TimeoutSeconds = 10
    )

    $Deadline = (Get-Date).AddSeconds($TimeoutSeconds)
    do {
        $OpenPorts = @($Ports | Where-Object {
            -not (Test-TcpPortClosed -Port ([int]$_))
        })
        if ($OpenPorts.Count -eq 0) {
            return $true
        }
        Start-Sleep -Milliseconds 100
    } while ((Get-Date) -lt $Deadline)

    Write-Warning "fixture端口仍可连接: $($OpenPorts -join ', ')"
    return $false
}

function Write-StopSentinel {
    param(
        [Parameter(Mandatory)][string]$StopPath,
        [Parameter(Mandatory)][string]$RunId
    )

    if (Test-Path -LiteralPath $StopPath) {
        Assert-NotReparsePoint -Path $StopPath -Label 'stop sentinel'
        $StopItem = Get-Item -Force -LiteralPath $StopPath
        if ($StopItem.PSIsContainer -or
            (Get-Content -Raw -LiteralPath $StopPath).Trim() -cne $RunId) {
            throw "fixture stop sentinel验证失败: $StopPath"
        }
        return
    }

    New-AtomicTextFile -Path $StopPath -Content $RunId
}

function Complete-TombstoneCleanup {
    param(
        [Parameter(Mandatory)][string]$TombstonePath,
        [Parameter(Mandatory)][string]$PointerPath,
        [switch]$AllowLiveOwner
    )

    $Cleanup = Get-CleanupTombstone -TombstonePath $TombstonePath
    if (-not $AllowLiveOwner -and
        (Test-OwnerProcessIsCurrent -State $Cleanup.OwnerState)) {
        throw "fixture owner仍在运行(pid=$($Cleanup.OwnerState.owner_pid))；拒绝cleanup"
    }
    if (-not (Wait-FixturePortsClosed -Ports $Cleanup.Ports)) {
        throw 'fixture server端口尚未关闭；保留tombstone且不kill进程'
    }

    Assert-PointerMatches `
        -PointerPath $PointerPath `
        -Root $Cleanup.Root `
        -RunId $Cleanup.RunId
    if (Test-Path -LiteralPath $PointerPath) {
        Remove-Item -LiteralPath $PointerPath -Force
    }

    if (Test-Path -LiteralPath $Cleanup.Root) {
        if (-not (Test-Path -LiteralPath $Cleanup.Root -PathType Container)) {
            throw "fixture root不是目录；保留tombstone: $($Cleanup.Root)"
        }
        Assert-NotReparsePoint `
            -Path $Cleanup.Root `
            -Label 'cleanup fixture root'
        Remove-Item -LiteralPath $Cleanup.Root -Recurse -Force
    }

    $FinalManifest = Get-CleanupTombstone -TombstonePath $TombstonePath
    if (-not $FinalManifest.Root.Equals(
        $Cleanup.Root,
        [System.StringComparison]::OrdinalIgnoreCase
    ) -or $FinalManifest.RunId -cne $Cleanup.RunId) {
        throw 'cleanup tombstone在删除期间发生变化，拒绝移除'
    }
    Remove-Item -LiteralPath $TombstonePath -Force
}

function Remove-OwnedFixture {
    param(
        [Parameter(Mandatory)]$Owned,
        [Parameter(Mandatory)][string]$PointerPath,
        [Parameter(Mandatory)][string]$TombstonePath
    )

    $Verified = Get-OwnedFixture -PointerPath $PointerPath
    if (-not $Verified.Root.Equals(
        [string]$Owned.Root,
        [System.StringComparison]::OrdinalIgnoreCase
    ) -or $Verified.RunId -cne [string]$Owned.RunId) {
        throw 'cleanup前fixture ownership发生变化，拒绝递归删除'
    }
    if (-not (Wait-FixturePortsClosed -Ports @($Verified.State.ports))) {
        throw 'fixture server端口尚未关闭；保留现场且不kill进程'
    }

    $Manifest = [ordered]@{
        version = 1
        root = $Verified.Root
        run_id = $Verified.RunId
        ports = @(Get-ValidatedPorts `
            -State $Verified.State `
            -Label 'fixture state before cleanup')
        owner_pid = [long]$Verified.State.owner_pid
        owner_start_utc_ticks = [long]$Verified.State.owner_start_utc_ticks
        created_utc = [DateTimeOffset]::UtcNow.ToString('O')
    } | ConvertTo-Json -Compress
    New-AtomicTextFile -Path $TombstonePath -Content $Manifest

    $Created = Get-CleanupTombstone -TombstonePath $TombstonePath
    if (-not $Created.Root.Equals(
        $Verified.Root,
        [System.StringComparison]::OrdinalIgnoreCase
    ) -or $Created.RunId -cne $Verified.RunId) {
        throw '新建cleanup tombstone未通过identity校验'
    }
    Complete-TombstoneCleanup `
        -TombstonePath $TombstonePath `
        -PointerPath $PointerPath `
        -AllowLiveOwner
}

function Read-JsonLines {
    param([Parameter(Mandatory)][string]$Path)

    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) {
        return @()
    }

    foreach ($Line in Get-Content -LiteralPath $Path) {
        if (-not [string]::IsNullOrWhiteSpace($Line)) {
            $Line | ConvertFrom-Json
        }
    }
}

function Test-KindSequence {
    param(
        [Parameter(Mandatory)][object[]]$Rows,
        [Parameter(Mandatory)][string[]]$Expected
    )

    if ($Rows.Count -ne $Expected.Count) {
        return $false
    }
    for ($Index = 0; $Index -lt $Expected.Count; $Index++) {
        if ([string]$Rows[$Index].kind -cne $Expected[$Index]) {
            return $false
        }
    }
    return $true
}

function Show-Oracle {
    param(
        [Parameter(Mandatory)][string]$Name,
        [Parameter(Mandatory)][bool]$Passed,
        [Parameter(Mandatory)][string]$Success,
        [Parameter(Mandatory)][string]$Failure
    )

    $script:OracleResults.Add([pscustomobject]@{
        Name = $Name
        Passed = $Passed
        Detail = if ($Passed) { $Success } else { $Failure }
    })
    if ($Passed) {
        Write-Host "[AUTO PASS] $Name - $Success" -ForegroundColor Green
    } else {
        Write-Host "[AUTO FAIL] $Name - $Failure" -ForegroundColor Red
    }
}

function Write-OracleSummary {
    Write-Phase 'AUTO检查汇总'
    foreach ($Result in $script:OracleResults) {
        $Prefix = if ($Result.Passed) { '[PASS]' } else { '[FAIL]' }
        $Color = if ($Result.Passed) { 'Green' } else { 'Red' }
        Write-Host "$Prefix $($Result.Name) - $($Result.Detail)" -ForegroundColor $Color
    }
    $FailureCount = @($script:OracleResults | Where-Object {
        -not $_.Passed
    }).Count
    Write-Host "合计: $($script:OracleResults.Count)项，失败: ${FailureCount}项"
    return $FailureCount
}

function Test-FinalVerdict {
    param(
        [Parameter(Mandatory)][object[]]$Rows,
        [Parameter(Mandatory)][string]$Kind,
        [Parameter(Mandatory)][string]$Verdict
    )

    $Matches = @($Rows | Where-Object { [string]$_.kind -ceq $Kind })
    if ($Matches.Count -ne 1) {
        return $false
    }
    $Row = $Matches[0]
    $OkProperty = $Row.PSObject.Properties['ok']
    $VerdictProperty = $Row.PSObject.Properties['verdict']
    return $null -ne $OkProperty -and
        $OkProperty.Value -is [bool] -and
        $OkProperty.Value -eq $true -and
        $null -ne $VerdictProperty -and
        [string]$VerdictProperty.Value -ceq $Verdict
}

function Test-LogTuple {
    param(
        [Parameter(Mandatory)][object[]]$Rows,
        [Parameter(Mandatory)][string]$Marker,
        [Parameter(Mandatory)][string]$Model
    )

    return @($Rows | Where-Object {
        [string]$_.marker -cne $Marker -or
        [string]$_.model -cne $Model
    }).Count -eq 0
}

function Invoke-Mysteries {
    param(
        [Parameter(Mandatory)][string]$Exe,
        [Parameter(Mandatory)][string]$Workspace,
        [string[]]$Arguments = @()
    )

    Push-Location -LiteralPath $Workspace
    try {
        & $Exe @Arguments
        if ($LASTEXITCODE -ne 0) {
            throw "mysteries退出码不是0: $LASTEXITCODE"
        }
    } finally {
        Pop-Location
    }
}

function Test-OwnerProcessIsCurrent {
    param([Parameter(Mandatory)]$State)

    try {
        $Owner = Get-Process -Id ([int]$State.owner_pid) -ErrorAction Stop
    } catch {
        return $false
    }

    $ActualTicks = $Owner.StartTime.ToUniversalTime().Ticks
    return $ActualTicks -eq [long]$State.owner_start_utc_ticks
}

$RepoRoot = [System.IO.Path]::GetFullPath(
    (Join-Path $PSScriptRoot '..\..\..')
)
$Exe = Join-Path $RepoRoot 'target\codex-readonly-subagent\release\mysteries.exe'
$ServerScript = Join-Path $PSScriptRoot 'manual-fixture-server.ps1'
$FixturePointer = Join-Path $env:TEMP $PointerName
$FixtureTombstone = Join-Path $env:TEMP $TombstoneName

if ($Action -eq 'CleanupStale') {
    if (Test-Path -LiteralPath $FixtureTombstone) {
        $Cleanup = Get-CleanupTombstone -TombstonePath $FixtureTombstone
        if (Test-OwnerProcessIsCurrent -State $Cleanup.OwnerState) {
            throw "fixture owner仍在运行(pid=$($Cleanup.OwnerState.owner_pid))；请回原验证窗口退出"
        }
        Assert-PointerMatches `
            -PointerPath $FixturePointer `
            -Root $Cleanup.Root `
            -RunId $Cleanup.RunId
        if (Test-Path -LiteralPath $Cleanup.Root -PathType Container) {
            Write-StopSentinel `
                -StopPath (Join-Path $Cleanup.Root 'fixture.stop') `
                -RunId $Cleanup.RunId
        }
        if (-not (Wait-FixturePortsClosed -Ports $Cleanup.Ports)) {
            throw 'fixture server未退出或端口仍被占用；tombstone现场已保留'
        }
        Complete-TombstoneCleanup `
            -TombstonePath $FixtureTombstone `
            -PointerPath $FixturePointer
        Write-Host '已从cleanup tombstone安全恢复并清理fixture。' -ForegroundColor Green
        return
    }

    if (-not (Test-Path -LiteralPath $FixturePointer)) {
        Write-Host '没有需要清理的stale fixture。' -ForegroundColor Green
        return
    }
    $PointerInfo = Get-FixturePointerInfo -PointerPath $FixturePointer
    if (-not $PointerInfo.RootExists) {
        Remove-Item -LiteralPath $FixturePointer -Force
        Write-Host '已清理边界验证通过的dangling fixture pointer。' -ForegroundColor Green
        return
    }

    $Owned = Get-OwnedFixture -PointerPath $FixturePointer
    if (Test-OwnerProcessIsCurrent -State $Owned.State) {
        throw "fixture owner仍在运行(pid=$($Owned.State.owner_pid))；请回原验证窗口退出"
    }

    $StopPath = Join-Path $Owned.Root 'fixture.stop'
    Write-StopSentinel -StopPath $StopPath -RunId $Owned.RunId
    if (-not (Wait-FixturePortsClosed -Ports @($Owned.State.ports))) {
        throw 'fixture server未退出或端口仍被占用；现场已保留'
    }
    Remove-OwnedFixture `
        -Owned $Owned `
        -PointerPath $FixturePointer `
        -TombstonePath $FixtureTombstone
    Write-Host '已安全清理stale fixture。' -ForegroundColor Green
    return
}

if (-not (Test-Path -LiteralPath $Exe -PathType Leaf)) {
    throw "release executable不存在: $Exe"
}
if (-not (Test-Path -LiteralPath $ServerScript -PathType Leaf)) {
    throw "fixture server脚本不存在: $ServerScript"
}
if ((Test-Path -LiteralPath $FixturePointer) -or
    (Test-Path -LiteralPath $FixtureTombstone)) {
    throw @"
发现未清理fixture状态:
  current: $FixturePointer
  tombstone: $FixtureTombstone
若原验证窗口仍在运行，请回该窗口正常退出。
若原窗口已关闭，执行:
pwsh -NoProfile -File '$PSCommandPath' -Action CleanupStale
"@
}

$FixtureRunId = [guid]::NewGuid().ToString('N')
$FixtureRoot = Join-Path $env:TEMP "$RootPrefix$FixtureRunId"
$OwnerSentinel = Join-Path $FixtureRoot '.fixture-owner'
$StatePath = Join-Path $FixtureRoot 'state.json'
$Workspace = Join-Path $FixtureRoot 'workspace'
$Outside = Join-Path $FixtureRoot 'outside'
$TestHome = Join-Path $FixtureRoot 'home'
$ConfigDir = Join-Path $TestHome '.config\mysteries'
$FixtureStop = Join-Path $FixtureRoot 'fixture.stop'
$StallLog = Join-Path $FixtureRoot 'stall.jsonl'
$MarkerALog = Join-Path $FixtureRoot 'marker-a.jsonl'
$MarkerBLog = Join-Path $FixtureRoot 'marker-b.jsonl'
$EscapeLog = Join-Path $FixtureRoot 'escape.jsonl'
$StallReady = Join-Path $FixtureRoot 'stall.ready'
$MarkerAReady = Join-Path $FixtureRoot 'marker-a.ready'
$MarkerBReady = Join-Path $FixtureRoot 'marker-b.ready'
$EscapeReady = Join-Path $FixtureRoot 'escape.ready'
$FixtureJobs = @()
$OwnedFixture = $null
$PointerAcquiredByThisRun = $false
$CleanupSucceeded = $false
$OldHome = $env:HOME
$OldUserProfile = $env:USERPROFILE
$IgnoreLinkCreated = $false
$IgnoreLinkSkipReason = '未执行§9.4'

try {
    New-Item -ItemType Directory -Path $FixtureRoot -ErrorAction Stop | Out-Null
    Assert-NotReparsePoint -Path $FixtureRoot -Label 'new fixture root'
    Set-Content -Encoding ascii -NoNewline -LiteralPath $OwnerSentinel $FixtureRunId

    New-Item -ItemType Directory -Force $Workspace, $Outside, $ConfigDir | Out-Null
    @($StallLog, $MarkerALog, $MarkerBLog, $EscapeLog) | ForEach-Object {
        New-Item -ItemType File -Path $_ -Force | Out-Null
    }

    function Get-FreePort {
        $Listener = [System.Net.Sockets.TcpListener]::new(
            [System.Net.IPAddress]::Loopback,
            0
        )
        try {
            $Listener.Start()
            return ([System.Net.IPEndPoint]$Listener.LocalEndpoint).Port
        } finally {
            $Listener.Stop()
        }
    }

    $Ports = [System.Collections.Generic.HashSet[int]]::new()
    while ($Ports.Count -lt 4) {
        [void]$Ports.Add((Get-FreePort))
    }
    $PortValues = @($Ports)
    $StallPort = $PortValues[0]
    $MarkerAPort = $PortValues[1]
    $MarkerBPort = $PortValues[2]
    $EscapePort = $PortValues[3]

    $OwnerProcess = [System.Diagnostics.Process]::GetCurrentProcess()
    [ordered]@{
        run_id = $FixtureRunId
        owner_pid = $PID
        owner_start_utc_ticks = $OwnerProcess.StartTime.ToUniversalTime().Ticks
        created_utc = [DateTimeOffset]::UtcNow.ToString('O')
        ports = @($StallPort, $MarkerAPort, $MarkerBPort, $EscapePort)
    } | ConvertTo-Json | Set-Content -Encoding utf8 -LiteralPath $StatePath
    New-AtomicTextFile -Path $FixturePointer -Content $FixtureRoot
    $PointerAcquiredByThisRun = $true
    if (Test-Path -LiteralPath $FixtureTombstone) {
        Assert-PointerMatches `
            -PointerPath $FixturePointer `
            -Root $FixtureRoot `
            -RunId $FixtureRunId
        Remove-Item -LiteralPath $FixturePointer -Force
        $PointerAcquiredByThisRun = $false
        throw 'pointer创建期间出现cleanup tombstone；本轮拒绝启动'
    }
    $OwnedFixture = Get-OwnedFixture -PointerPath $FixturePointer

    Set-Content -Encoding utf8 (Join-Path $Workspace 'safe.txt') 'ROOT-SAFE-CONTENT'
    $ForbiddenMarker = 'OUTSIDE-ONLY-7F3A91C2'
    Set-Content -Encoding utf8 (Join-Path $Outside 'marker.txt') $ForbiddenMarker
    $RootOnlyMarker = 'ROOT-OUTSIDE-SAFE-CONTENT'
    Set-Content -Encoding utf8 (Join-Path $Outside 'root-only.txt') $RootOnlyMarker
    $IgnoreProbeMarker = 'IGNORE-PARENT-VISIBLE'
    Set-Content -Encoding utf8 (
        Join-Path $Workspace 'ignore-parent-visible.txt'
    ) $IgnoreProbeMarker
    Set-Content -Encoding utf8 (
        Join-Path $FixtureRoot '.ignore'
    ) 'ignore-parent-visible.txt'
    $ExternalIgnoreRules = Join-Path $Outside 'external-ignore-rules'
    Set-Content -Encoding utf8 $ExternalIgnoreRules (
        "[$ForbiddenMarker`nignore-parent-visible.txt"
    )
    $IgnoreLink = Join-Path $Workspace '.gitignore'

    $OutsideMarker = (Resolve-Path (
        Join-Path $Outside 'marker.txt'
    )).Path
    $RootOnlyAbsolute = (Resolve-Path (
        Join-Path $Outside 'root-only.txt'
    )).Path
    $RelativeEscape = '..\outside\marker.txt'
    $JunctionEscape = 'outside-junction\marker.txt'
    $JunctionCreated = $true
    $JunctionSkipReason = ''
    try {
        New-Item -ItemType Junction `
            -Path (Join-Path $Workspace 'outside-junction') `
            -Target $Outside `
            -ErrorAction Stop | Out-Null
    } catch {
        $JunctionCreated = $false
        $JunctionSkipReason = $_.Exception.Message
    }

    function Write-FixtureConfig {
        param([Parameter(Mandatory)][string]$Active)

        @"
active = "$Active"
max_iterations = 12
timeout_secs = 300
thinking = "low"

[providers.stall]
kind = "openai"
base_url = "http://127.0.0.1:$StallPort/v1"
model = "stall-model"
auth_type = "api_key"

[providers.fixture_a]
kind = "openai"
base_url = "http://127.0.0.1:$MarkerAPort/v1"
model = "fixture-a-model"
auth_type = "api_key"

[providers.fixture_b]
kind = "openai"
base_url = "http://127.0.0.1:$MarkerBPort/v1"
model = "fixture-b-model"
auth_type = "api_key"

[providers.escape]
kind = "openai"
base_url = "http://127.0.0.1:$EscapePort/v1"
model = "escape-model"
auth_type = "api_key"
"@ | Set-Content -Encoding utf8 -LiteralPath (
            Join-Path $ConfigDir 'config.toml'
        )
    }

    $InitialActive = if ($Section -eq '9.4') { 'escape' } else { 'stall' }
    Write-FixtureConfig -Active $InitialActive
    @'
stall = local-fixture-only
fixture_a = local-fixture-only
fixture_b = local-fixture-only
escape = local-fixture-only
'@ | Set-Content -Encoding utf8 -LiteralPath (
        Join-Path $ConfigDir 'credentials'
    )

    # Start-Job继承当前环境；先隔离HOME，避免fixture读取用户config/credentials。
    $env:HOME = $TestHome
    $env:USERPROFILE = $TestHome
    $StallJob = Start-Job -FilePath $ServerScript -ArgumentList @(
        $StallPort, 'Stall', 'STALL', $StallLog, $StallReady, $FixtureStop,
        '', '', '', '', ''
    )
    $FixtureJobs += $StallJob
    $MarkerAJob = Start-Job -FilePath $ServerScript -ArgumentList @(
        $MarkerAPort, 'Marker', 'FIXTURE-A',
        $MarkerALog, $MarkerAReady, $FixtureStop,
        '', '', '', '', ''
    )
    $FixtureJobs += $MarkerAJob
    $MarkerBJob = Start-Job -FilePath $ServerScript -ArgumentList @(
        $MarkerBPort, 'Marker', 'FIXTURE-B',
        $MarkerBLog, $MarkerBReady, $FixtureStop,
        '', '', '', '', ''
    )
    $FixtureJobs += $MarkerBJob
    $EscapeJunctionArgument = if ($JunctionCreated) {
        $JunctionEscape
    } else {
        ''
    }
    $EscapeJob = Start-Job -FilePath $ServerScript -ArgumentList @(
        $EscapePort, 'Escape', 'ESCAPE',
        $EscapeLog, $EscapeReady, $FixtureStop,
        $OutsideMarker, $RelativeEscape, $EscapeJunctionArgument,
        $RootOnlyAbsolute, $ForbiddenMarker
    )
    $FixtureJobs += $EscapeJob

    $ReadyFiles = @($StallReady, $MarkerAReady, $MarkerBReady, $EscapeReady)
    $ReadyDeadline = (Get-Date).AddSeconds(10)
    while (@(
        $ReadyFiles | Where-Object {
            -not (Test-Path -LiteralPath $_)
        }
    ).Count -gt 0 -and (Get-Date) -lt $ReadyDeadline) {
        $FailedJobs = @($FixtureJobs | Where-Object { $_.State -eq 'Failed' })
        if ($FailedJobs.Count -gt 0) {
            $FailedJobs | Receive-Job
            throw 'fixture job启动失败'
        }
        Start-Sleep -Milliseconds 100
    }
    $MissingReady = @($ReadyFiles | Where-Object {
        -not (Test-Path -LiteralPath $_)
    })
    if ($MissingReady.Count -gt 0) {
        $FixtureJobs | Receive-Job
        throw "fixture未在10秒内全部启动: $($MissingReady -join ', ')"
    }

    if ($Action -eq 'Smoke') {
        $SmokeBody = [ordered]@{
            model = 'smoke-model'
            messages = @(
                [ordered]@{
                    role = 'user'
                    content = 'SMOKE'
                }
            )
            tools = @(
                [ordered]@{
                    type = 'function'
                    function = [ordered]@{
                        name = 'delegate_task'
                        description = 'smoke'
                        parameters = [ordered]@{
                            type = 'object'
                            properties = @{}
                        }
                    }
                }
            )
            stream = $true
        } | ConvertTo-Json -Depth 16 -Compress

        foreach ($Port in @(
            $StallPort,
            $MarkerAPort,
            $MarkerBPort,
            $EscapePort
        )) {
            $Response = Invoke-WebRequest `
                -Method Post `
                -Uri "http://127.0.0.1:$Port/v1/chat/completions" `
                -ContentType 'application/json' `
                -Body $SmokeBody
            if ($Response.StatusCode -ne 200 -or
                -not $Response.Content.Contains('delegate_task')) {
                throw "fixture smoke请求失败: port=$Port"
            }
        }
        Write-Host 'fixture smoke启动与请求验证成功。' -ForegroundColor Green
    } else {
        $Run93 = $Section -in @('All', '9.3')
        $Run94 = $Section -in @('All', '9.4')
        $Run95 = $Section -in @('All', '9.5')

        if ($Run93) {
            Write-FixtureConfig -Active 'stall'
            Write-Phase '§9.3A - stall child与Interrupt'
            $StallChildReady = "$StallReady.child"
            $StallWatcherJob = Start-Job -ScriptBlock {
                param($SignalPath, $StopPath)

                while (-not (Test-Path -LiteralPath $SignalPath) -and
                    -not (Test-Path -LiteralPath $StopPath)) {
                    Start-Sleep -Milliseconds 100
                }
                if (Test-Path -LiteralPath $SignalPath) {
                    try {
                        Add-Type -AssemblyName System.Windows.Forms
                        [void][System.Windows.Forms.MessageBox]::Show(
                            "child Provider已进入stall。`n现在回TUI只按一次Esc。",
                            '§9.3 stall ready',
                            [System.Windows.Forms.MessageBoxButtons]::OK,
                            [System.Windows.Forms.MessageBoxIcon]::Information,
                            [System.Windows.Forms.MessageBoxDefaultButton]::Button1,
                            [System.Windows.Forms.MessageBoxOptions]::ServiceNotification
                        )
                    } catch {
                        try {
                            Add-Type -AssemblyName System.Windows.Extensions
                            [System.Media.SystemSounds]::Exclamation.Play()
                        } catch {
                            # ready文件仍由主脚本在TUI退出后作AUTO oracle。
                        }
                    }
                }
            } -ArgumentList $StallChildReady, $FixtureStop
            $FixtureJobs += $StallWatcherJob

            Write-Host @'
TUI启动后只做这些操作：
  1. 输入 STALL-INTERRUPT-CHECK
  2. 不要凭手速按Esc；等“§9.3 stall ready”弹窗出现
  3. 关闭弹窗，回TUI只按一次Esc
  4. 目测：只出现一次“已中断本轮”；outer卡为Error；无迟到Done/text/status
  5. 输入 RECOVERY-NO-DELEGATE，应得到 STALL-RECOVERY-OK
  6. 输入 /exit
'@
            [void](Read-Host '按Enter启动TUI')
            Invoke-Mysteries -Exe $Exe -Workspace $Workspace

            $StallRows = @(Read-JsonLines -Path $StallLog)
            $StallReadyObserved = Test-Path -LiteralPath $StallChildReady -PathType Leaf
            $StallBasePassed = $StallReadyObserved -and
                (Test-KindSequence `
                    -Rows $StallRows `
                    -Expected @('outer-delegate', 'child-stalled', 'recovery')) -and
                (Test-LogTuple `
                    -Rows $StallRows `
                    -Marker 'STALL' `
                    -Model 'stall-model') -and
                @(Read-JsonLines -Path $EscapeLog).Count -eq 0 -and
                @(Read-JsonLines -Path $MarkerALog).Count -eq 0 -and
                @(Read-JsonLines -Path $MarkerBLog).Count -eq 0
            Show-Oracle `
                -Name '§9.3 stall日志与ready信号' `
                -Passed $StallBasePassed `
                -Success '真实ready信号已出现，stall请求序列与tuple精确匹配' `
                -Failure '未观察到ready信号、序列/tuple错误或串到其他endpoint'
            if (-not $StallBasePassed) {
                $StallRows | Format-Table request, marker, model, kind, ok, verdict
            }

            Write-Phase '§9.3B - --continue恢复同一中断session'
            $BeforeContinue = @{
                Stall = $StallRows.Count
                Escape = @(Read-JsonLines -Path $EscapeLog).Count
                A = @(Read-JsonLines -Path $MarkerALog).Count
                B = @(Read-JsonLines -Path $MarkerBLog).Count
            }
            Write-Host @'
恢复后：
  1. 提交Prompt前确认outer卡无Running残留、无重复ToolResult或child内部卡、没有自动重跑
  2. 输入 RECOVERY-NO-DELEGATE，应得到 STALL-RECOVERY-OK
  3. 输入 /exit
'@
            [void](Read-Host '按Enter启动 --continue')
            Invoke-Mysteries `
                -Exe $Exe `
                -Workspace $Workspace `
                -Arguments @('--continue')

            $AfterContinueStall = @(Read-JsonLines -Path $StallLog)
            $ContinueAdded = @(
                $AfterContinueStall | Select-Object -Skip $BeforeContinue.Stall
            )
            $ContinuePassed = $ContinueAdded.Count -eq 1 -and
                [string]$ContinueAdded[0].kind -ceq 'recovery' -and
                [string]$ContinueAdded[0].marker -ceq 'STALL' -and
                [string]$ContinueAdded[0].model -ceq 'stall-model' -and
                @(Read-JsonLines -Path $EscapeLog).Count -eq $BeforeContinue.Escape -and
                @(Read-JsonLines -Path $MarkerALog).Count -eq $BeforeContinue.A -and
                @(Read-JsonLines -Path $MarkerBLog).Count -eq $BeforeContinue.B
            Show-Oracle `
                -Name '§9.3 --continue四日志' `
                -Passed $ContinuePassed `
                -Success '只在stall endpoint新增一次正确tuple的recovery' `
                -Failure '出现child重跑、串endpoint/model或缺少recovery'

            Write-Phase '§9.3C - picker --resume恢复含Interrupted的session'
            $BeforeResume93 = @{
                Stall = $AfterContinueStall.Count
                Escape = @(Read-JsonLines -Path $EscapeLog).Count
                A = @(Read-JsonLines -Path $MarkerALog).Count
                B = @(Read-JsonLines -Path $MarkerBLog).Count
            }
            Write-Host @'
picker打开后：
  1. 选择刚才含“已中断本轮”的stall session
  2. 提交Prompt前确认outer卡无Running残留、无重复ToolResult或child内部卡、没有自动重跑
  3. 输入 RECOVERY-NO-DELEGATE，应得到 STALL-RECOVERY-OK
  4. 输入 /exit
'@
            [void](Read-Host '按Enter启动 --resume')
            Invoke-Mysteries `
                -Exe $Exe `
                -Workspace $Workspace `
                -Arguments @('--resume')

            $AfterResume93Stall = @(Read-JsonLines -Path $StallLog)
            $Resume93Added = @(
                $AfterResume93Stall | Select-Object -Skip $BeforeResume93.Stall
            )
            $Resume93Passed = $Resume93Added.Count -eq 1 -and
                [string]$Resume93Added[0].kind -ceq 'recovery' -and
                [string]$Resume93Added[0].marker -ceq 'STALL' -and
                [string]$Resume93Added[0].model -ceq 'stall-model' -and
                @(Read-JsonLines -Path $EscapeLog).Count -eq $BeforeResume93.Escape -and
                @(Read-JsonLines -Path $MarkerALog).Count -eq $BeforeResume93.A -and
                @(Read-JsonLines -Path $MarkerBLog).Count -eq $BeforeResume93.B
            Show-Oracle `
                -Name '§9.3 picker --resume四日志' `
                -Passed $Resume93Passed `
                -Success '中断session恢复后未重跑child，只新增一次recovery' `
                -Failure '恢复了错误session、出现自动重跑/串endpoint或缺少recovery'
        }

        if ($Run94) {
            Write-FixtureConfig -Active 'escape'
            Write-Phase '§9.4 - containment与ignore（脚本自动执行）'
            Write-Host @"
下面3个Prompt由脚本自动运行；若能创建file symlink，再自动运行第4个。
你不需要打开TUI或执行/models。
第一项outer必须返回ESCAPE-OUTER-OK，delegate日志必须为ESCAPE-OK。
任何结果都不得泄漏marker：$ForbiddenMarker
"@
            if (-not $JunctionCreated) {
                Write-Warning "junction子项平台skip: $JunctionSkipReason"
            }

            Invoke-Mysteries `
                -Exe $Exe `
                -Workspace $Workspace `
                -Arguments @('--headless', 'ESCAPE-CHECK')
            Invoke-Mysteries `
                -Exe $Exe `
                -Workspace $Workspace `
                -Arguments @('--headless', 'ROOT-READ-CHECK')
            Invoke-Mysteries `
                -Exe $Exe `
                -Workspace $Workspace `
                -Arguments @('--headless', 'IGNORE-PARENT-CHECK')

            $IgnoreLinkCreated = $true
            $IgnoreLinkSkipReason = ''
            try {
                New-Item -ItemType SymbolicLink `
                    -Path $IgnoreLink `
                    -Target $ExternalIgnoreRules `
                    -ErrorAction Stop | Out-Null
            } catch {
                $IgnoreLinkCreated = $false
                $IgnoreLinkSkipReason = $_.Exception.Message
                Write-Warning "file symlink子项平台skip: $IgnoreLinkSkipReason"
            }
            if ($IgnoreLinkCreated) {
                Invoke-Mysteries `
                    -Exe $Exe `
                    -Workspace $Workspace `
                    -Arguments @('--headless', 'IGNORE-LINK-CHECK')
            }

            $EscapeRows = @(Read-JsonLines -Path $EscapeLog)
            $ExpectedEscapeKinds = @(
                'outer-delegate',
                'child-read-attempts',
                'child-final',
                'outer-final',
                'root-read',
                'root-read-final',
                'ignore-parent-outer',
                'ignore-parent-child',
                'ignore-parent-child-final',
                'ignore-parent-outer-final'
            )
            if ($IgnoreLinkCreated) {
                $ExpectedEscapeKinds += @(
                    'ignore-link-outer',
                    'ignore-link-child',
                    'ignore-link-child-final',
                    'ignore-link-outer-final'
                )
            }
            $EscapePassed =
                (Test-KindSequence `
                    -Rows $EscapeRows `
                    -Expected $ExpectedEscapeKinds) -and
                (Test-LogTuple `
                    -Rows $EscapeRows `
                    -Marker 'ESCAPE' `
                    -Model 'escape-model') -and
                (Test-FinalVerdict `
                    -Rows $EscapeRows `
                    -Kind 'child-final' `
                    -Verdict 'ESCAPE-OK') -and
                (Test-FinalVerdict `
                    -Rows $EscapeRows `
                    -Kind 'outer-final' `
                    -Verdict 'ESCAPE-OUTER-OK') -and
                (Test-FinalVerdict `
                    -Rows $EscapeRows `
                    -Kind 'root-read-final' `
                    -Verdict 'ROOT-READ-OK') -and
                (Test-FinalVerdict `
                    -Rows $EscapeRows `
                    -Kind 'ignore-parent-child-final' `
                    -Verdict 'IGNORE-PARENT-OK') -and
                (Test-FinalVerdict `
                    -Rows $EscapeRows `
                    -Kind 'ignore-parent-outer-final' `
                    -Verdict 'IGNORE-PARENT-OUTER-OK')
            if ($IgnoreLinkCreated) {
                $EscapePassed = $EscapePassed -and
                    (Test-FinalVerdict `
                        -Rows $EscapeRows `
                        -Kind 'ignore-link-child-final' `
                        -Verdict 'IGNORE-LINK-OK') -and
                    (Test-FinalVerdict `
                        -Rows $EscapeRows `
                        -Kind 'ignore-link-outer-final' `
                        -Verdict 'IGNORE-LINK-OUTER-OK')
            }
            Show-Oracle `
                -Name '§9.4精确序列与verdict' `
                -Passed $EscapePassed `
                -Success '全部kind顺序、endpoint/model、child/outer verdict均精确匹配' `
                -Failure 'kind顺序、tuple或ok/verdict至少一项不匹配'
            if (-not $EscapePassed) {
                $EscapeRows | Format-Table request, marker, model, kind, ok, verdict
            }
        }

        if ($Run95) {
            Write-Phase '§9.5A - Provider/model pair与/model'
            Write-Host @'
只开这一个TUI，依次执行：
  1. /models 选择 fixture_a / fixture-a-model
  2. 输入 PAIR-A-CHECK，预期FIXTURE-A-OUTER-OK
  3. /models 选择 fixture_b / fixture-b-model
  4. 输入 PAIR-B-CHECK，预期只出现FIXTURE-B
  5. 输入 /model fixture-b-model-v2
  6. 输入 MODEL-B-V2-CHECK，预期仍只出现FIXTURE-B
  7. 输入 /exit
'@
            [void](Read-Host '按Enter启动§9.5 TUI')
            Invoke-Mysteries -Exe $Exe -Workspace $Workspace

            $MarkerARows = @(Read-JsonLines -Path $MarkerALog)
            $MarkerBRows = @(Read-JsonLines -Path $MarkerBLog)
            $Triple = @('outer-delegate', 'child-final', 'outer-final')
            $MarkerAPassed =
                (Test-KindSequence -Rows $MarkerARows -Expected $Triple) -and
                (Test-LogTuple `
                    -Rows $MarkerARows `
                    -Marker 'FIXTURE-A' `
                    -Model 'fixture-a-model')
            $MarkerBPassed = $MarkerBRows.Count -eq 6 -and
                (Test-KindSequence `
                    -Rows @($MarkerBRows | Select-Object -First 3) `
                    -Expected $Triple) -and
                (Test-KindSequence `
                    -Rows @($MarkerBRows | Select-Object -Skip 3) `
                    -Expected $Triple) -and
                (Test-LogTuple `
                    -Rows @($MarkerBRows | Select-Object -First 3) `
                    -Marker 'FIXTURE-B' `
                    -Model 'fixture-b-model') -and
                (Test-LogTuple `
                    -Rows @($MarkerBRows | Select-Object -Skip 3) `
                    -Marker 'FIXTURE-B' `
                    -Model 'fixture-b-model-v2')
            Show-Oracle `
                -Name '§9.5 pair/model日志' `
                -Passed ($MarkerAPassed -and $MarkerBPassed) `
                -Success 'A→B pair完整，/model只改变B的model字段' `
                -Failure 'endpoint marker与model组合或请求序列不符合预期'

            Write-Phase '§9.5B - picker --resume'
            $BeforeResume95 = @{
                Stall = @(Read-JsonLines -Path $StallLog).Count
                Escape = @(Read-JsonLines -Path $EscapeLog).Count
                A = $MarkerARows.Count
                B = $MarkerBRows.Count
            }
            Write-Host @'
picker打开后：
  1. 选择刚完成的fixture_b成功session
  2. 提交Prompt前确认无Running残留、无重复ToolResult或child内部卡、没有自动重跑
  3. 输入 RECOVERY-NO-DELEGATE，应得到 FIXTURE-B-RECOVERY-OK
  4. 输入 /exit
'@
            [void](Read-Host '按Enter启动 --resume')
            Invoke-Mysteries `
                -Exe $Exe `
                -Workspace $Workspace `
                -Arguments @('--resume')

            $AfterResume95Stall = @(Read-JsonLines -Path $StallLog)
            $AfterResume95Escape = @(Read-JsonLines -Path $EscapeLog)
            $AfterResume95A = @(Read-JsonLines -Path $MarkerALog)
            $AfterResume95B = @(Read-JsonLines -Path $MarkerBLog)
            $Resume95Added = @(
                $AfterResume95B | Select-Object -Skip $BeforeResume95.B
            )
            $Resume95Passed =
                $AfterResume95Stall.Count -eq $BeforeResume95.Stall -and
                $AfterResume95Escape.Count -eq $BeforeResume95.Escape -and
                $AfterResume95A.Count -eq $BeforeResume95.A -and
                $Resume95Added.Count -eq 1 -and
                [string]$Resume95Added[0].kind -ceq 'recovery' -and
                [string]$Resume95Added[0].marker -ceq 'FIXTURE-B' -and
                [string]$Resume95Added[0].model -ceq 'fixture-b-model-v2'
            Show-Oracle `
                -Name '§9.5 picker --resume四日志' `
                -Passed $Resume95Passed `
                -Success '恢复未重跑child，只新增一次B/v2 recovery' `
                -Failure '恢复期间出现自动重跑、串Provider/model或缺少recovery'
        }

        Write-Phase '人工结果回报'
        $ManualReport = [System.Collections.Generic.List[string]]::new()
        $ManualReport.Add('把最后的AUTO检查汇总和下面人工观察发给实施Agent：')
        if ($Run93) {
            $ManualReport.Add('  §9.3 Interrupt UI：PASS / FAIL')
        }
        if ($Run94) {
            $ManualReport.Add(
                "  §9.4 junction：$(if ($JunctionCreated) { 'PASS' } else { "SKIP - $JunctionSkipReason" })"
            )
            $ManualReport.Add(
                "  §9.4 file symlink：$(if ($IgnoreLinkCreated) { 'PASS' } else { "SKIP - $IgnoreLinkSkipReason" })"
            )
        }
        if ($Run95) {
            $ManualReport.Add('  §9.5 picker/model UI：PASS / FAIL')
        }
        Write-Host ($ManualReport -join [Environment]::NewLine)
    }
} finally {
    if ($null -eq $OldHome) {
        Remove-Item Env:HOME -ErrorAction SilentlyContinue
    } else {
        $env:HOME = $OldHome
    }
    if ($null -eq $OldUserProfile) {
        Remove-Item Env:USERPROFILE -ErrorAction SilentlyContinue
    } else {
        $env:USERPROFILE = $OldUserProfile
    }

    if ($null -ne $OwnedFixture) {
        try {
            Write-StopSentinel `
                -StopPath $FixtureStop `
                -RunId $FixtureRunId

            $StopDeadline = (Get-Date).AddSeconds(10)
            while (@(
                $FixtureJobs | Where-Object {
                    $TerminalJobStates -notcontains [string]$_.State
                }
            ).Count -gt 0 -and (Get-Date) -lt $StopDeadline) {
                Start-Sleep -Milliseconds 100
            }

            $LiveJobs = @($FixtureJobs | Where-Object {
                $TerminalJobStates -notcontains [string]$_.State
            })
            if ($LiveJobs.Count -gt 0) {
                $LiveJobs | Format-Table Id, State, Name
                throw 'fixture未在10秒内停止；保留现场且不kill进程'
            }

            $FixtureJobs | Receive-Job -ErrorAction SilentlyContinue
            $FixtureJobs | Remove-Job -ErrorAction SilentlyContinue
            if (-not (Wait-FixturePortsClosed -Ports @(
                $OwnedFixture.State.ports
            ))) {
                throw 'fixture server端口未关闭；保留现场且不kill进程'
            }
            Remove-OwnedFixture `
                -Owned $OwnedFixture `
                -PointerPath $FixturePointer `
                -TombstonePath $FixtureTombstone
            $CleanupSucceeded = $true
            Write-Host 'fixture已安全清理。' -ForegroundColor Green
        } catch {
            Write-Warning $_.Exception.Message
            Write-Warning @"
自动cleanup未完成，现场已保留。原验证进程结束后运行：
pwsh -NoProfile -File '$PSCommandPath' -Action CleanupStale
"@
        }
    } elseif (Test-Path -LiteralPath $FixtureRoot) {
        try {
            $ResolvedRoot = [System.IO.Path]::GetFullPath($FixtureRoot)
            $ResolvedTemp = [System.IO.Path]::GetFullPath($env:TEMP)
            $SafeEarlyRoot =
                $FixtureRunId -cmatch '^[0-9a-f]{32}$' -and
                [System.IO.Path]::GetDirectoryName($ResolvedRoot).Equals(
                    $ResolvedTemp,
                    [System.StringComparison]::OrdinalIgnoreCase
                ) -and
                [System.IO.Path]::GetFileName($ResolvedRoot).Equals(
                    "$RootPrefix$FixtureRunId",
                    [System.StringComparison]::Ordinal
                )
            if (-not $SafeEarlyRoot) {
                throw 'early fixture root不在严格TEMP/GUID边界'
            }
            Assert-NotReparsePoint -Path $ResolvedRoot -Label 'early fixture root'
            if (-not (Test-Path -LiteralPath $OwnerSentinel -PathType Leaf)) {
                throw 'early fixture缺少owner sentinel，拒绝递归删除'
            }
            Assert-NotReparsePoint `
                -Path $OwnerSentinel `
                -Label 'early owner sentinel'
            if ((Get-Content -Raw -LiteralPath $OwnerSentinel).Trim() -cne
                $FixtureRunId) {
                throw 'early fixture owner token不匹配，拒绝递归删除'
            }

            $EarlyRemovedByProtocol = $false
            if ($PointerAcquiredByThisRun -and
                (Test-Path -LiteralPath $FixturePointer)) {
                $EarlyPointer = Get-FixturePointerInfo `
                    -PointerPath $FixturePointer
                $PointerStillOurs = $EarlyPointer.Root.Equals(
                    $ResolvedRoot,
                    [System.StringComparison]::OrdinalIgnoreCase
                ) -and $EarlyPointer.RunId -ceq $FixtureRunId
                if ($PointerStillOurs) {
                    $EarlyOwned = Get-OwnedFixture `
                        -PointerPath $FixturePointer
                    if (-not (Wait-FixturePortsClosed -Ports @(
                        $EarlyOwned.State.ports
                    ))) {
                        throw 'early fixture端口未关闭；保留现场'
                    }
                    Remove-OwnedFixture `
                        -Owned $EarlyOwned `
                        -PointerPath $FixturePointer `
                        -TombstonePath $FixtureTombstone
                    $EarlyRemovedByProtocol = $true
                }
            }
            if (-not $EarlyRemovedByProtocol) {
                # 未赢得pointer（或pointer已不再属于本轮）时绝不跟随它；
                # Start-Job尚未发生，只删除已严格验证的本轮root。
                Remove-Item -LiteralPath $ResolvedRoot -Recurse -Force
            }
            $CleanupSucceeded = $true
        } catch {
            Write-Warning $_.Exception.Message
            Write-Warning 'early cleanup未完成；现场已保留且未递归reparse root。'
        }
    }
}

$OracleFailureCount = 0
if ($Action -eq 'Run') {
    $OracleFailureCount = Write-OracleSummary
}
if (-not $CleanupSucceeded) {
    throw '真机核验cleanup未完成'
}
if ($OracleFailureCount -gt 0) {
    throw "真机核验有${OracleFailureCount}项AUTO检查失败"
}
