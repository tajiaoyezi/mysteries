# modernize-ratatui-and-deny-unsound 真机验收

> 验收状态：用户于 2026-07-11 确认 8.1–8.3 全部通过，未报告失败项。未提供的环境版本与截图不作推断。

## 0. 测试对象与准备

- `BASE_SHA`: `950420d4252d4fc98e1f87ab15c22cb12552e8b0`
- release binary: `H:\devlopment\code\wps\mysteries\target\codex-ratatui-modernize\release\mysteries.exe`
- 自动化基线：`cargo clippy --all-targets --locked -- -D warnings`、`cargo test --locked`、`cargo build --release --locked`、strict OpenSpec validation 均已通过。
- 快照：仅 `mysteries__tui__render__tests__tui_command_completion.snap` 为用户已批准差异，无 `.snap.new`。

在 PowerShell 7 中执行：

```powershell
$exe = 'H:\devlopment\code\wps\mysteries\target\codex-ratatui-modernize\release\mysteries.exe'
Test-Path -LiteralPath $exe
& $exe --version
```

预期：`Test-Path` 为 `True`，版本命令正常退出。

## 8.1 TUI 渲染、输入、粘贴、滚动与终端生命周期

先准备一段大粘贴并启动 TUI：

```powershell
$paste = (1..30 | ForEach-Object { 'CONPTY_PASTE_LINE_{0:D2} 中文 mixed-width text' -f $_ }) -join [Environment]::NewLine
Set-Clipboard -Value $paste
& $exe
```

- [x] Welcome 与四区布局正常，没有错位、空白区或 panic。
- [x] Midnight token、边框、C10 状态行、C11 输入框与迁移前一致。
- [x] 输入中文、ASCII、左右移动与 Backspace 后，光标位置和宽字符显示正确。
- [x] 输入提示词“只回复一段含标题、列表、Rust code block 与未知语言 zzz code block 的 markdown”，Assistant markdown 与未知语言 fallback 正常。
- [x] 在输入框按 `Ctrl+V` 粘贴准备好的 30 行文本：不会逐行误提交；出现大粘贴折叠；尾流收口后仍能继续键入 `-TAIL`。
- [x] 按 Enter 后提交内容完整，没有首字符泄漏、重复行或残余 Enter。
- [x] 模型输出足够长后，鼠标滚轮与 `↑` / `↓` / `PgUp` / `PgDn` 均可滚动；滚轮后方向键仍有效。
- [x] 鼠标拖选可复制文本，释放后状态正常。
- [x] 让模型同轮并行调用 5 个 `grep` 搜索 `H:\devlopment\code` 的不存在 pattern；Running 卡出现后立即按 Esc，中断能收口且下一轮 `read_file Cargo.toml` 可正常执行。
- [x] 最后连续按两次 `Ctrl+C` 退出；PowerShell 恢复可见光标、普通输入、滚轮和选择，没有残留 raw mode / alternate screen / mouse capture。

结果记录：

```text
Windows Terminal / PowerShell 版本：未提供
通过项：本节全部检查项
失败项与复现步骤：无
截图或备注：用户于 2026-07-11 确认验证完成；未提供截图
```

## 8.2 C6 权限矩阵

每次使用下面的有效 Network prompt：

```text
必须调用 web_fetch 获取 https://example.com/，不要调用其他工具。
```

- [x] Normal：出现可授权 C6；参数、target、redirect/SSRF scope 完整；按 `n` 拒绝后继续工作。
- [x] AcceptEdits：仍出现可授权 C6；按 Esc 拒绝后继续工作。
- [x] Plan：仍出现可授权 C6；按 `n` 拒绝后继续工作。
- [x] Yolo：相同有效调用自动放行，不出现 C6，`https://example.com/` 获取成功。

Yolo 下再输入：

```text
必须调用 web_fetch，并把 url 参数严格设置为 JSON 数字 123，不得修正。
```

- [x] 出现 `reject-only` C6，包含 `args: {"url":123}` 与 `reason: missing or invalid url`。
- [x] 该请求没有允许选项，按 `n` 或 Esc 可关闭，工具没有执行。
- [x] 权限框关闭后，模型输出滚动、方向键、Esc 中断与下一轮输入仍正常。

结果记录：

```text
Normal：通过（用户确认）
AcceptEdits：通过（用户确认）
Plan：通过（用户确认）
Yolo authorizable：通过（用户确认）
Yolo reject-only：通过（用户确认）
失败项与复现步骤：无
```

## 8.3 auth login 的 selector、隐藏输入与 raw-mode 恢复

在同一个 PowerShell 会话先记录配置与凭据 hash：

```powershell
$config = Join-Path $HOME '.config\mysteries\config.toml'
$credentials = Join-Path $HOME '.config\mysteries\credentials'
$beforeConfig = if (Test-Path $config) { (Get-FileHash $config -Algorithm SHA256).Hash } else { '<missing>' }
$beforeCredentials = if (Test-Path $credentials) { (Get-FileHash $credentials -Algorithm SHA256).Hash } else { '<missing>' }
```

操作 A：

```powershell
& $exe auth login
```

- [x] 用 `↑` / `↓` 多次移动，首尾环绕且每次只移动一格；Enter 可进入下一步。
- [x] 在任一 selector 按 Esc 取消，立即返回 PowerShell，不写配置或凭据。

操作 B：再次运行 `& $exe auth login`，选择任一预设 provider 进入 API key 隐藏输入：

- [x] 输入 `abc` 只显示遮罩，不出现明文；每个按键只输入一次，没有 Release 重复。
- [x] Backspace / Delete 正常删除；按 Ctrl+C 取消并返回 PowerShell。
- [x] 退出后执行 `Read-Host 'raw mode 恢复检查'` 可正常输入、回显和回车。

复核文件未变：

```powershell
$afterConfig = if (Test-Path $config) { (Get-FileHash $config -Algorithm SHA256).Hash } else { '<missing>' }
$afterCredentials = if (Test-Path $credentials) { (Get-FileHash $credentials -Algorithm SHA256).Hash } else { '<missing>' }
"CONFIG_UNCHANGED=$($beforeConfig -eq $afterConfig)"
"CREDENTIALS_UNCHANGED=$($beforeCredentials -eq $afterCredentials)"
```

- [x] 两项均为 `True`。

结果记录：

```text
selector：通过（用户确认）
隐藏输入：通过（用户确认）
Esc / Ctrl+C：通过（用户确认）
raw mode：通过（用户确认）
CONFIG_UNCHANGED：True（用户确认）
CREDENTIALS_UNCHANGED：True（用户确认）
失败项与复现步骤：无
```

## 8.5–8.6 远端与合入后门禁

- PR：`#4`，实现 merge SHA：`e82cf2a455838f6063c28932b1d35dfba4f20740`。
- PR 最新 head 的 Windows CI、Ubuntu CI 与 Security audit 全部通过。
- `master` push 的 Security audit run：`29183880624`，job：`86626446906`，结论：success。
- merge SHA 的 audit log 确认固定安装 `cargo-audit 0.22.2`，通过绝对 `$AUDIT_BIN` 执行 `audit --deny unsound --file "$GITHUB_WORKSPACE/Cargo.lock"`，扫描根 `Cargo.lock` 并成功退出。
- audit 保留唯一 allowed warning `RUSTSEC-2025-0141`（`syntect -> bincode 1.3.3` unmaintained）；据命令策略与 exit 0 得出 0 vulnerability / 0 unsound，不宣称 warning-free。
- 已知 GitHub Actions Node20→Node24 兼容警告仍按设计留给独立 `modernize-github-actions-runtime` change，不属于本次阻塞。

## 最终结论

- [x] 8.1 全部通过。
- [x] 8.2 全部通过。
- [x] 8.3 全部通过。
- [x] 可以进入提交 / PR / 远端 CI 阶段。
- [x] 8.5 PR 最新 head 的 Windows / Ubuntu CI 与 Security audit 全部通过。
- [x] 8.6 实现 merge SHA 的 `master` Security audit 通过，可以进入 OpenSpec archive 准备阶段。
