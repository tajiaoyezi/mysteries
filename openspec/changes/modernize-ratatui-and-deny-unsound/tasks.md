执行边界：本 change 只迁移 `ratatui 0.29 -> 0.30.x` 与其要求的单一 `crossterm 0.28 -> 0.29`、移除旧 `paste` / unsound `lru` 路径并启用 `--deny unsound`。不主动采用新的输入语义，不治理 `bincode`，不改 GitHub Actions runtime，不改 Agent 内核。UI 只允许经审查并由用户批准的 `mysteries__tui__render__tests__tui_command_completion.snap` 迁移差异，不为复刻 0.29 的 `/models` 缺字/留白编写兼容 hack，也不得扩张到其他视觉改造。该工作属于 TUI 外壳与依赖安全迁移，不走 headless 强制 RED→GREEN；迁移前 audit 失败仅作为安全策略 RED 证据。所有会编译、检查、测试、clippy 或 build 的 Cargo 命令均在各自 PowerShell shell 显式设置 `CARGO_TARGET_DIR=target/codex-ratatui-modernize`；`cargo tree` / `cargo audit` / `cargo fmt` 不产出 target。不得 kill 用户进程或切回默认 target 规避锁。

## 1. 保存迁移前事实基线

- [x] 1.1 确认当前实现基线：记录 `BASE_SHA = git rev-parse HEAD`、`rustc --version`、`cargo --version`、`Cargo.toml` 的 `ratatui 0.29` / `crossterm 0.28`、`git status --short` 与全部已跟踪 `.snap` 清单；若开始前已有用户改动，先区分并保护，不覆盖。
- [x] 1.2 记录旧依赖图：运行 `cargo tree --locked -i ratatui`、`cargo tree --locked -i paste`、`cargo tree --locked -i lru`、`cargo tree --locked -i crossterm` 与 `cargo tree --locked -d`，证明 `paste 1.0.15`、`lru 0.12.5` 仅经 `ratatui 0.29.0` 引入。
- [x] 1.3 保存安全 RED：在 PowerShell 解析并记录 `$AUDIT_EXE = (Get-Command cargo-audit -CommandType Application).Source` 及其完整版本，运行 `& $AUDIT_EXE audit --file Cargo.lock`；规划时预期为 0 vulnerability / 3 allowed warning。再运行 `& $AUDIT_EXE audit --deny unsound --file Cargo.lock`，规划时预期仅因 `RUSTSEC-2026-0002` 非零退出并含 `ratatui -> lru` 路径。若 live RustSec database 的数量、分类或路径已漂移，立即停止并回到 proposal/spec 重评范围，禁止忽略新 advisory 或硬套旧计数。
- [x] 1.4 设置 `$env:CARGO_TARGET_DIR='target/codex-ratatui-modernize'`，运行 `cargo test --locked --lib tui::` 与 `cargo test --locked --test e2e`，确认迁移前 TUI / 集成基线全绿、无 `.snap.new`、已跟踪快照零 diff。

## 2. 定向迁移 Ratatui 依赖图

- [x] 2.1 将 `Cargo.toml` 的 Ratatui 声明改为 `version = "0.30"`、`default-features = false`、直接 features 恰为 `crossterm_0_29` / `layout-cache`（不重复 opt-in 未直接使用的 `underline-color`）；把直接 crossterm 改为 `0.29` 并继续启用 crate defaults + `event-stream`，不新增 direct crate。记录 0.29 defaults 相比 0.28 新增 `derive-more`，不得写成 default feature 集逐字不变。
- [x] 2.2 只对 Ratatui / Crossterm 做定向 lockfile 更新，解析当前兼容的 Ratatui 0.30 patch 与 crossterm 0.29；审查 `Cargo.lock` diff，确认变更只来自两者迁移、0.30 模块化和显式 feature 组合，并把 `derive_more` / `derive_more-impl` 标记为 crossterm 0.29 已接受的上游传递变化，撤销任何无关 dependency update。
- [x] 2.3 运行 `cargo tree --locked` / `cargo tree --locked -d` 及针对 `ratatui`、`crossterm`、`paste`、`lru` 的查询，确认只含 `ratatui 0.30.x` 与单一 `crossterm 0.29.x`，不存在 `ratatui 0.29`、`crossterm 0.28`、`paste 1.0.15`、`lru 0.12.5`，并记录新 layout-cache 的安全 `lru` 路径。
- [x] 2.4 重新解析绝对 `$AUDIT_EXE` 并运行 `& $AUDIT_EXE audit --deny unsound --file Cargo.lock`，规划时预期为 0 vulnerability / 0 unsound、exit 0，并仍保留 `syntect -> bincode 1.3.3` unmaintained warning。零值由命令策略与 exit 0 证明，不要求原始输出含零计数；若 live database 出现其他 vulnerability / unsound，停止并重评，其他 allowed warning 如实记录，报告不得写成 warning-free。

## 3. 完成最小 Ratatui 0.30 / Crossterm 0.29 API 与运行路径兼容

- [x] 3.1 在独立 PowerShell shell 设置 `$env:CARGO_TARGET_DIR='target/codex-ratatui-modernize'` 后运行 `cargo check --all-targets --locked`，整理编译器实际暴露的 Ratatui 0.30 / Crossterm 0.29 breaking points；再设置 `$env:INSTA_UPDATE='no'` 并运行 `cargo test --locked --lib tui::`，记录编译器未暴露但回归测试暴露的 Buffer 差异，不得预先重构未报错模块。
- [x] 3.2 只在 `src/tui/` 与直接使用 crossterm 的 `src/cli.rs` 实际兼容点适配模块/trait/type/API；每一处非机械改动须能映射到 `tui-shell` 既有 requirement、CLI 交互认证等价或经批准的单份快照迁移，禁止借机改布局、宽度算法、状态机、文案、颜色、键位、Event::Paste 策略或终端事件路由。若 `src/cli.rs` 无编译改动，也必须保留其现有按键映射测试并进入真机验收。
- [x] 3.3 在独立 PowerShell shell 重新设置隔离 `CARGO_TARGET_DIR` 并运行 `cargo check --all-targets --locked`，再审查 `git diff -- src/tui src/cli.rs`：只保留 0.30/0.29 兼容所需改动；Agent / Provider / Tool / Permission / Session / Config 与 CLI flags/持久化行为不得被顺手修改。

## 4. 加固 unsound 审计策略

- [x] 4.1 将 `.github/workflows/security-audit.yml` 的绝对 binary 调用精确改为 `audit --deny unsound --file "$GITHUB_WORKSPACE/Cargo.lock"`；固定 `cargo-audit` 版本、checkout SHA、隔离目录、preflight、权限、trigger 与 `persist-credentials: false` 全部保持不变。
- [x] 4.2 在 PowerShell 解析 `$AUDIT_EXE = (Get-Command cargo-audit -CommandType Application).Source` 并记录其绝对路径；正向运行 `& $AUDIT_EXE audit --deny unsound --file Cargo.lock`，再把 §1.1 的 `git show <BASE_SHA>:Cargo.lock` 经 stdin 交给同一绝对 binary 的 `audit --deny unsound --file -` 做旧 lockfile负向验证。正向必须 exit 0，负向必须因 `RUSTSEC-2026-0002` 非零退出；不得使用可被项目 `[alias] audit` shadow 的 `cargo audit` 作为 alias 隔离证据。
- [x] 4.3 静态审查 workflow diff：除新增 `--deny unsound` 参数外无 Action version/runtime、cache、权限或事件语义变化；确认没有 `.cargo/audit.toml`、advisory ignore、输出过滤或 `continue-on-error`。

## 5. TUI / CLI 自动化回归与受控快照迁移

- [x] 5.1 在每个独立 PowerShell shell 设置隔离 `CARGO_TARGET_DIR`，分别运行 `cargo test --locked --lib tui::theme`、`tui::width`、`tui::render`、`tui::app`、`tui::input_batch`（以 test filter 逐项执行），覆盖 token、Unicode width、Buffer、viewport、Press/Release、event batch、选择复制、输入/粘贴与权限 modal；`src/tui/terminal.rs` 当前无 test seam，不得用 `tui::terminal` 的 0 tests 冒充 mouse/raw/alt-screen 覆盖。
- [x] 5.2 在独立 PowerShell shell 设置隔离 `CARGO_TARGET_DIR` 与 `INSTA_UPDATE=new` 并运行完整 `cargo test --locked --lib tui::`，核对 `设计规范/01-设计令牌.md`、`02-布局与交互.md` 与 `03-组件清单.md` C1–C11。预期只有 `tui::render::tests::command_completion_snapshot` 因 0.30 Buffer 差异产生 `.snap.new`；任何其他测试/快照失败都必须先修复并重新生成证据，不能纳入允许清单。
- [x] 5.3 展示并逐行审查 `mysteries__tui__render__tests__tui_command_completion.snap.new`：允许内容仅为 `/model` 相邻同 style run 合并，以及 `/models` 从旧版缺字/留白变为完整命令描述和随之产生的覆盖位置变化；区域、行数、token、其他命令或文本不得变化。取得用户对这份精确 diff 的显式批准且确认没有其他 `.snap.new` 后，只运行 `cargo insta accept --snapshot mysteries__tui__render__tests__tui_command_completion.snap` 定向接受该文件；随后确认 `git diff --name-only <BASE_SHA> -- src/tui/snapshots` 只列该快照且递归无 `.snap.new`，禁止批量 approve 未审查基线。
- [x] 5.4 若编译器适配触及 Buffer / selection / layout seam，补最小事后 characterization test 并运行 mutation check（临时反转关键判断应使测试失败）；若未触及则记录“不需要新增测试”的代码证据，不为凑数量添加同义测试。
- [x] 5.5 在独立 PowerShell shell 设置隔离 `CARGO_TARGET_DIR`，运行 `cargo test --locked --lib cli::tests::apply_secret_key`、`cargo test --locked --lib cli::tests::apply_select_key`、`cargo test --locked --lib cli::tests::run_auth_login` 与 `cargo test --locked --lib cli::tests::run_auth_logout`，确认 Press/Release、方向键环绕、Enter、Backspace、Esc/Ctrl+C、取消不写文件与 credential 映射保持全绿；隐藏输入显示和 raw-mode 恢复仍由 §8.3 真机验证，不得用纯逻辑测试冒充。

## 6. 同步用户与贡献者文档

- [x] 6.1 更新 `CONTRIBUTING.md` 本地依赖审计命令为带 `--deny unsound` 的真实可执行命令，并说明 vulnerability / unsound hard-fail、unmaintained report-only。
- [x] 6.2 更新 README 工程质量/安全 workflow 说明：Ratatui 0.30 最小 features、0 vulnerability / 0 unsound 门禁，以及剩余 unmaintained warning 不等于 warning-free；不改功能宣传与截图。
- [x] 6.3 更新 CHANGELOG `[Unreleased]`：记录 Ratatui 0.30 安全迁移、`paste` / 旧 `lru` 路径移除和 `--deny unsound`；保留 `bincode` warning，并且不预写 release/tag/archive 数量。

## 7. 完整本地质量门禁

- [x] 7.1 运行 `cargo fmt --all`，随后 `cargo fmt --all -- --check` 与 `git diff --check`；审查实际改动文件，没有无关格式 churn。
- [x] 7.2 在每个独立 PowerShell shell 设置 `$env:CARGO_TARGET_DIR='target/codex-ratatui-modernize'`，依次运行 `cargo clippy --all-targets --locked -- -D warnings`、`cargo test --locked`、`cargo build --release --locked`；不得 kill 用户进程或回退默认 target。
- [x] 7.3 重跑依赖最终证据：`cargo tree --locked -d`、Ratatui/crossterm/paste/lru 反向查询，以及重新解析绝对 `$AUDIT_EXE` 后执行 `& $AUDIT_EXE audit --deny unsound --file Cargo.lock`；结果必须与 §2 契约一致。
- [x] 7.4 运行 `openspec validate modernize-ratatui-and-deny-unsound --strict` 与 `openspec validate --all --strict`，确认 change 与全部主 specs 通过；重查快照 diff 只含经批准的 `mysteries__tui__render__tests__tui_command_completion.snap` 且无 `.snap.new`。

## 8. 真机与远端验收

- [x] 8.1 基于隔离 target 的 release binary 编写 `manual-verification.md`，在 Windows Terminal 真机启动 TUI，检查 welcome/四区、Midnight token、输入光标与中文宽字符、Assistant markdown/未知代码语言、滚轮与方向键滚动、鼠标选择复制；执行 ConPTY 多行和大段粘贴，确认不误提交、折叠/尾流收口正确且粘贴后键盘继续输入。实际结果逐项记录，不能用快照冒充真机。
- [x] 8.2 真机在 Normal / AcceptEdits / Plan 下分别触发一次可授权 C6 并用 `n` / `Esc` 拒绝；在 Yolo 下先确认同类 authorizable 调用自动放行且不出现 C6，再用 invalid Network preview 触发 `reject-only` C6、确认不可授权并以 `n` / `Esc` 关闭。随后验证模型输出后仍可滚动、Esc 中断可恢复、mouse capture / raw mode / alt-screen 在双 Ctrl+C 退出后完整恢复；所有行为和布局应与迁移前一致。
- [x] 8.3 在 Windows Terminal 真机进入 `auth login` 的交互式 provider/model selector 与隐藏 key 输入，验证方向键、Enter、Backspace、隐藏字符回显和非 Press 事件不会重复输入；分别用 Esc / Ctrl+C 取消，确认没有持久化测试凭据、raw mode 完整恢复且 PowerShell 可立即继续输入。
- [x] 8.4 提交/推送前复核改动范围、完整本地门禁、仅一份经用户批准的快照 diff 与真机记录；未经用户授权不得执行 Git 写操作。
- [ ] 8.5 PR 上等待最新 head 的 Windows + Ubuntu `CI` 与 `Security audit` 全部通过；检查 audit log 真实执行绝对 binary 的 `audit --deny unsound`、扫描根 lockfile并 exit 0，同时保留允许的 unmaintained warning。报告可据命令策略与 exit 0 得出 0 vulnerability / 0 unsound，但不得声称原始 log 含未实际输出的零计数，也不能用旧 SHA 的 green 代替。
- [ ] 8.6 合入后在 `master` 对最终 merge SHA 运行或确认 `Security audit`（含 `workflow_dispatch`），再次验证相同策略与结果；全部证据齐备后才允许进入 OpenSpec archive，archive 时按 AGENTS.md 起草并经用户审阅 change 决策记录。
