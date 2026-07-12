# 真机验证操作手册 — `harden-dependency-security`

> 只验证本 change 的依赖收敛、markdown 无回归和远端 RustSec workflow。
> 自动化门禁通过后由用户亲测，或由用户显式委托实施 agent 执行；只有掌握真实命令、真机 UI 或远端 job 证据的一方才能勾选对应 §10 项。
> 本清单不要求人为重新引入 vulnerability，也不要求修改或 approve 任何快照。
> 本文件 10.1–10.5 与 `tasks.md` §10 一一对应，是唯一人工验证步骤定义；`tasks.md` §10 是唯一状态源。
> 完成一项后只更新 `tasks.md` §10 的对应 checkbox；本文件中的 `[ ]` 仅用于抄录结果，不形成第二套进度状态。

## 准备

在仓库根目录打开 PowerShell 7：

```powershell
Set-Location 'H:\devlopment\code\wps\mysteries'
$auditBin = (Get-Command cargo-audit -ErrorAction Stop).Source
$auditVersion = & $auditBin --version
if ($auditVersion -ne 'cargo-audit 0.22.2') { throw "需要 cargo-audit 0.22.2，当前为: $auditVersion" }
git status --short --branch
$lock = Get-Item -LiteralPath 'Cargo.lock'
if ($lock.PSIsContainer -or $lock.LinkType) { throw 'Cargo.lock 必须是 regular non-symlink file' }
git ls-files --error-unmatch -- Cargo.lock
if ((((git ls-files -s -- Cargo.lock) -split '\s+')[0]) -ne '100644') { throw 'Cargo.lock Git mode 必须为 100644' }
$auditHome = if ($env:CARGO_HOME) { $env:CARGO_HOME } else { Join-Path $HOME '.cargo' }
Test-Path -LiteralPath '.cargo\audit.toml'
Test-Path -LiteralPath (Join-Path $auditHome 'audit.toml')
```

开始前应满足：

- implementation tasks 已完成，本地 full test / release build 通过；
- 本地 `cargo-audit` 完整版本输出精确为 `cargo-audit 0.22.2`；若不是，先执行 `cargo install cargo-audit --version 0.22.2 --locked --force`；
- `Cargo.toml`、regular non-symlink 且 tracked mode=`100644` 的 `Cargo.lock`、`.github/workflows/security-audit.yml` 已是待验证版本；
- 上述两个 `Test-Path` 都输出 `False`，即项目根与有效 `CARGO_HOME` 均不存在 `audit.toml`；
- 要验证 10.4–10.5 时，包含该 workflow 的分支已 push 并创建 PR；
- `git status` 中没有意外的 `.snap.new` 或既有 `.snap` 变化。

## 勾选总表

```text
[ ] 10.1  本地 cargo-audit：0 vulnerability，剩余 warning 如实可见
[ ] 10.2  依赖树：crossbeam 已修复，未使用 loader 链已消失
[ ] 10.3  TUI markdown：Rust 高亮与未知语言 fallback 无回归
[ ] 10.4  PR security-audit：独立 job 首次绿灯
[ ] 10.5  workflow_dispatch：合入后可手动运行且仍为绿灯
```

---

## 10.1 本地 RustSec 结果

### 你要做的

```powershell
$auditBin = (Get-Command cargo-audit -ErrorAction Stop).Source
& $auditBin audit --file Cargo.lock
$LASTEXITCODE
```

### 预期效果

- `$LASTEXITCODE` 为 `0`；
- 命令退出成功且输出中没有 vulnerability 条目（人工记录为 0 vulnerability），不得再出现：
  - `RUSTSEC-2026-0204`
  - `RUSTSEC-2026-0194`
  - `RUSTSEC-2026-0195`
- warning 仍然可见，按规划基线预计包含：
  - `RUSTSEC-2025-0141`：`bincode 1.3.3` unmaintained
  - `RUSTSEC-2024-0436`：`paste 1.0.15` unmaintained
  - `RUSTSEC-2026-0002`：`lru 0.12.5` unsound
- 不应再出现 `yaml-rust` 的 `RUSTSEC-2024-0320`。

如果 RustSec database 在实施后新增了 advisory，不要按旧数字强行判定通过：保留完整输出并交回审查。

**本项勾选：** `[ ] 10.1`

---

## 10.2 依赖树收敛

### 你要做的

```powershell
cargo tree --locked -i crossbeam-epoch@0.9.20
cargo tree --locked -i plist
cargo tree --locked -i quick-xml
cargo tree --locked -i yaml-rust
cargo tree --locked -i bincode@1.3.3
cargo tree --locked -i paste@1.0.15
cargo tree --locked -i lru@0.12.5
```

### 预期效果

| 命令 | 预期 |
|---|---|
| `crossbeam-epoch@0.9.20` | `cargo tree -i` 自上而下显示 `crossbeam-epoch 0.9.20 -> crossbeam-deque -> ignore -> mysteries` |
| `plist` / `quick-xml` / `yaml-rust` | Cargo 报 package ID 未匹配，表示已不在依赖图 |
| `bincode@1.3.3` | 仍只经 `syntect` 引入 |
| `paste@1.0.15` / `lru@0.12.5` | 仍只经 `ratatui 0.29` 引入，与本 change 非目标一致 |

另执行：

```powershell
git diff -- Cargo.toml Cargo.lock
```

确认没有 `ratatui` 升级、`rust-version`、`[patch.crates-io]`、advisory ignore、项目级 / `CARGO_HOME` `audit.toml` 或无关依赖升级。

**本项勾选：** `[ ] 10.2`

---

## 10.3 TUI markdown 冒烟

### 你要做的

1. 直接启动已通过 `tasks.md` 7.3 locked release build 的 binary，不再调用 Cargo 重新解析依赖：

   ```powershell
   & '.\target\release\mysteries.exe'
   ```

2. 发送以下 prompt：

   ```text
   不调用工具。请严格用 markdown 回答：先写一个标题，再给一个 rust 围栏代码块，内容包含 fn main() 与 println!；最后给一个语言标记为 zzz 的围栏代码块。
   ```

3. 观察两个代码块，然后正常退出 TUI。

### 预期效果

- Rust 代码块有语言标签和 token 级不同颜色，`fn` 与普通标识符可区分；
- `zzz` 未知语言块使用统一正文色和代码块底色，不 panic、不空白；
- 标题、代码块底色、滚动与退出后的 terminal 恢复均与变更前一致；
- 没有缺少 syntax/theme 资源或启动 panic。

Daylight 主题由自动化 `TestBackend + insta` 快照覆盖；当前产品未提供运行时主题切换，因此真机不伪造该入口。

**本项勾选：** `[ ] 10.3`

---

## 10.4 PR 上的 security-audit 首次绿灯

### 你要做的

1. push 含本 change 实现的分支并创建或更新 PR。
2. 打开 GitHub PR 的 Checks。
3. 找到独立的 **Security audit** workflow、**RustSec dependency audit** job（job id 为 `security-audit`）并展开日志。

### 预期效果

- **Security audit** 只运行一个 Ubuntu job，不复制进 Windows + Ubuntu build matrix；
- checkout 使用固定 SHA 且日志/配置可确认 `persist-credentials: false`；
- 日志显示在 `$RUNNER_TEMP` 下新建隔离 `CARGO_HOME` / install root，并固定安装 `cargo-audit 0.22.2`；
- 日志显示先确认 `Cargo.lock` 是 regular non-symlink、已跟踪且 Git mode=`100644`，项目根无 `.cargo/audit.toml`；随后以绝对 `cargo-audit` binary 完整断言版本，并执行 `audit --file <absolute Cargo.lock>`，命令成功且没有 vulnerability 条目；
- workflow 日志中不得出现用来执行审计的 `cargo audit` dispatch；仓库 `.cargo/config.toml [alias] audit` 不得影响 gate；
- 3 个剩余 warning 可见但 job 为 green；
- 原有 fmt/clippy/test/build CI 仍独立运行；`security-audit` 没有 paths filter，在普通代码 PR 上也会产生稳定 check。

若工具安装、版本断言、RustSec advisory database network/fetch/load 或 lockfile preflight/解析失败，job 应为 red；crates.io index/yanked network failure 只应产生显式 best-effort warning。重跑前先保留错误日志，不把真正的 hard-fail 当成允许跳过的 flaky check。

**本项勾选：** `[ ] 10.4`

---

## 10.5 合入后的 workflow_dispatch

> GitHub 通常要求带 `workflow_dispatch` 的 workflow 已存在于 default branch；因此本项在合入后验证，不阻塞本地 apply 完成。

### 你要做的

1. 合入后打开仓库 GitHub Actions 页面。
2. 选择 **Security audit** workflow。
3. 点击 **Run workflow**，选择 `master` 并启动。
4. 展开完成后的 job 日志。

### 预期效果

- 手动入口可见且能启动；
- 权限、`persist-credentials: false`、隔离目录、固定工具版本、绝对 binary、database 更新和审计命令与 PR job 一致；
- 结果为 0 vulnerability、剩余 warning 可见、job green；
- workflow 文件中仍有每周 schedule；手动与定时触发使用相同的 preflight 和审计命令。

**本项勾选：** `[ ] 10.5`

---

## 通过标准

- 10.1–10.3 通过：允许用户授权提交 / push；此前只能表述为 `local apply ready`。
- 10.4 通过：允许 merge；此前只能表述为“本地与 PR 前置验证完成”。
- 10.5 通过：允许 archive；只有此时才可表述为 change 完整验证。
