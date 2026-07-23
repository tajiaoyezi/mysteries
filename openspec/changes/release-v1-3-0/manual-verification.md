# release-v1-3-0 真机核验

本文件只保存可复用 procedure 与占位符，不回填真实 SHA、run ID、Release metadata 或截图；持久化证据统一进入 archive 决策记录。

所有 PowerShell 片段都先定位 repository；从 active change 或 archive change 目录执行均可：

```powershell
$Repo = (git rev-parse --show-toplevel).Trim()
if ($LASTEXITCODE -ne 0) { throw '当前目录不属于 Git repository' }
Set-Location $Repo
$Exe = Join-Path $Repo 'target\release\mysteries.exe'
```

## 1. 实现阶段自动门禁

```powershell
cargo fmt --all -- --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
cargo build --release --locked
& $Exe --version
& $Exe --help
cargo-audit audit --deny unsound --file Cargo.lock
openspec validate release-v1-3-0 --strict
openspec validate --all --strict
git diff --check
```

- [ ] 全部命令成功；RustSec 为 0 vulnerability / 0 unsound，允许的 unmaintained warning 仍如实显示。
- [ ] 71 份既有 TUI snapshot 只发生 `v1.2.0` → `v1.3.0` 字面量迁移，`.snap.new` 为 0。

## 2. Implementation PR bundle

把占位符替换为本次 PR Release run ID：

```powershell
$RunId = '<PR_RELEASE_RUN_ID>'
gh run download $RunId --name release-bundle-1.3.0 --dir "$env:TEMP\mysteries-pr-bundle"
Get-ChildItem "$env:TEMP\mysteries-pr-bundle"
Get-FileHash "$env:TEMP\mysteries-pr-bundle\mysteries-v1.3.0-x86_64-pc-windows-msvc.zip" -Algorithm SHA256
Get-FileHash "$env:TEMP\mysteries-pr-bundle\mysteries-v1.3.0-x86_64-unknown-linux-gnu.tar.gz" -Algorithm SHA256
```

- [ ] bundle 仅含两个 versioned archives、`SHA256SUMS` 与 `release-notes.md`；它只是 workflow artifact，不是公开 Release。

## 3. Repository settings（独立 admin 授权）

只有在明确批准 repository admin mutation 后才配置；这份批准不包含 merge、tag 或 deployment：

- [ ] immutable releases=`enabled:true`。
- [ ] `protect-master` 与 `protect-stable-tags` ruleset 精确匹配 change spec，且无常驻 bypass。
- [ ] `release` environment：reviewer=`tajiaoyezi`、`prevent_self_review=false`、`custom_branch_policies=true`。
- [ ] deployment policy 总数为 1，唯一 policy 是 `name=v1.3.0,type=tag`；UI 中 admin bypass 已关闭。

只读重验：

```powershell
$Repository = (gh repo view --json nameWithOwner --jq .nameWithOwner).Trim()
gh api "repos/$Repository/immutable-releases"
gh api "repos/$Repository/environments/release"
gh api --paginate "repos/$Repository/environments/release/deployment-branch-policies?per_page=100"
gh api --paginate "repos/$Repository/rulesets?includes_parents=false&per_page=100"
```

## 4. Master dry-run 与 tag（分别授权）

- [ ] implementation merge 后，`workflow_dispatch` 使用 `ref=master`，run `head_sha` 精确等于候选 merge SHA。
- [ ] CI、Security 与 Release dry-run 的精确 jobs 全部满足 change spec；下载并离线复核 `release-bundle-1.3.0`。
- [ ] 展示候选 SHA、evidence `run_id/run_attempt`、settings 与 UTC 日期后，单独批准创建并 push annotated `v1.3.0`；不得复用 implementation/settings 批准。

## 5. Protected environment deployment（独立批准）

tag run 等待审批后走最短 UI 路径：

1. 打开精确 `v1.3.0` tag run。
2. 点击 **Review deployments**。
3. 只选择 **release**。
4. 点击 **Approve and deploy**。

- [ ] 只批准该精确 run/attempt 的 deployment；tag 创建授权不得复用。
- [ ] tag run 任一步失败或取消即保留 run、tag 与可能的非公开 draft，并把当前 version/tag 视为已消耗；不得删除、移动、重建、公开或 rerun 复用，只能另起 patch release change。

## 6. 公开 Release 与 Windows TUI

```powershell
$Release = Invoke-RestMethod 'https://api.github.com/repos/tajiaoyezi/mysteries/releases/latest'
if ($Release.tag_name -ne 'v1.3.0' -or -not $Release.immutable) { throw '公开 Release metadata 异常' }
$DownloadDir = Join-Path $env:TEMP "mysteries-v1.3.0-$([guid]::NewGuid().ToString('N'))"
New-Item -ItemType Directory -Path $DownloadDir | Out-Null
$Asset = 'mysteries-v1.3.0-x86_64-pc-windows-msvc.zip'
$Base = 'https://github.com/tajiaoyezi/mysteries/releases/download/v1.3.0'
$Archive = Join-Path $DownloadDir $Asset
$Manifest = Join-Path $DownloadDir 'SHA256SUMS'
Invoke-WebRequest "$Base/$Asset" -OutFile $Archive
Invoke-WebRequest "$Base/SHA256SUMS" -OutFile $Manifest
$Matches = @(Get-Content $Manifest | Where-Object { $_ -match "^[0-9a-f]{64}  $([regex]::Escape($Asset))$" })
if ($Matches.Count -ne 1) { throw 'SHA256SUMS 记录缺失或重复' }
$Expected = ($Matches[0] -split '\s+')[0]
$Actual = (Get-FileHash $Archive -Algorithm SHA256).Hash.ToLowerInvariant()
if ($Actual -ne $Expected) { throw 'SHA-256 不匹配' }

$AppDir = Join-Path $DownloadDir 'app'
Expand-Archive $Archive -DestinationPath $AppDir
$Exe = Join-Path $AppDir 'mysteries.exe'
$SmokeHome = Join-Path $DownloadDir 'smoke-home'
New-Item -ItemType Directory -Path $SmokeHome | Out-Null
$SavedHome = $env:HOME
$SavedUserProfile = $env:USERPROFILE
$SavedXdgConfigHome = $env:XDG_CONFIG_HOME
try {
    $env:HOME = $SmokeHome
    $env:USERPROFILE = $SmokeHome
    $env:XDG_CONFIG_HOME = $SmokeHome
    Push-Location $SmokeHome
    try {
        $Version = (& $Exe --version | Out-String).Trim()
        if ($LASTEXITCODE -ne 0 -or $Version -ne 'mysteries 1.3.0') { throw "--version 异常: $Version" }
        & $Exe --help | Out-Null
        if ($LASTEXITCODE -ne 0) { throw '--help 失败' }
        & $Exe
        if ($LASTEXITCODE -ne 0) { throw 'TUI 非正常退出' }
    }
    finally {
        Pop-Location
    }
}
finally {
    $env:HOME = $SavedHome
    $env:USERPROFILE = $SavedUserProfile
    $env:XDG_CONFIG_HOME = $SavedXdgConfigHome
}
```

- [ ] checksum 匹配，`--version` 为 `mysteries 1.3.0`，`--help` 正常。
- [ ] Windows Terminal 中 TUI 正常启动并退出，PowerShell 立即恢复输入；未污染真实 credential/session。
