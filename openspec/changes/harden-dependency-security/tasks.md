## 1. 安全基线与实施边界

- [x] 1.1 在不修改依赖的基线上直接运行 `cargo-audit audit --file Cargo.lock`，保存 3 个 vulnerability（`RUSTSEC-2026-0204`、`RUSTSEC-2026-0194`、`RUSTSEC-2026-0195`）和 4 个 allowed warning 的完整 RED 输出、退出码及 advisory database 更新时间；若现场结果漂移，先按 code / tool 输出更新风险判断，不静默沿用规划数字。
- [x] 1.2 分别运行 `cargo tree --locked -i crossbeam-epoch@0.9.18`、`cargo tree --locked -i quick-xml@0.39.4`、`cargo tree --locked -i plist@1.9.0`、`cargo tree --locked -i yaml-rust@0.4.5`、`cargo tree --locked -i bincode@1.3.3`、`cargo tree --locked -i paste@1.0.15`、`cargo tree --locked -i lru@0.12.5`，保存修复前反向依赖路径且不得改写 lockfile。
- [x] 1.3 在 `src/tui/markdown.rs` 的 `#[cfg(test)]` 区域新增未闭合 Rust fence characterization：`render_markdown("```rust\nfn ", ...)` 不 panic、已到达的 `fn` 仍出现在代码块输出中；先在旧 feature 基线上运行并记录通过，锁定既有行为。
- [x] 1.4 确认本 change 不新增 headless 内核行为、无需强制 TDD 接口停点；除 1.3 的 test-only 变更外不修改 `src/`。若实际必须改 Rust runtime 逻辑或 UI 行为，立即停止并先修订 proposal/design/spec。

## 2. 收窄 manifest 与修复 lockfile

- [x] 2.1 将 `Cargo.toml` 的 `syntect` 从 `default-fancy` 收窄为 `default-syntaxes`、`default-themes`、`regex-fancy`，保持 `default-features = false`；不启用 `plist-load`、`yaml-load`、`html`、`regex-onig`，不新增 crate 或 `rust-version`。
- [x] 2.2 执行 `cargo update -p crossbeam-epoch --precise 0.9.20`，让 Cargo 同步按新的 `syntect` feature graph 收敛 `Cargo.lock`；不得运行无界或递归全量 `cargo update`。
- [x] 2.3 审查 `Cargo.toml` / `Cargo.lock` diff，确认只包含 `syntect` feature 收窄、`crossbeam-epoch 0.9.20` 及由此必需的传递依赖移除；预期 loader-only crate 还包括 `linked-hash-map`、`time`、`time-core`、`deranged`、`num-conv`、`powerfmt`，但不得把关闭 `html` feature 误报为同名 crate 移除，也不得添加 advisory ignore、`[patch.crates-io]`、vendor、输出过滤或无关版本 churn。

## 3. 依赖图与 RustSec 验收

- [x] 3.1 用 `cargo tree --locked -i crossbeam-epoch@0.9.20` 证明修复版本仍只经 `ignore -> crossbeam-deque` 引入，并确认旧 `crossbeam-epoch 0.9.18` 不在依赖图。
- [x] 3.2 用 `cargo tree --locked -i plist`、`cargo tree --locked -i quick-xml`、`cargo tree --locked -i yaml-rust` 的“package ID 未匹配”结果证明未使用 loader 链已移除；同时确认 `bincode` 仍只经 `syntect` 引入，`paste` / `lru 0.12.5` 仍只经 `ratatui 0.29` 引入。
- [x] 3.3 直接运行 `cargo-audit audit --file Cargo.lock`，要求退出码 0、没有 vulnerability 条目；逐项保存并报告全部 allowed warning（按已批准设计预计为 `bincode`、`paste`、`lru`），不得把结果表述为 0 advisory 或 warning-free；若 crates.io index 失败，明确其只影响 best-effort yanked 检查，不虚构 hard-fail。
- [x] 3.4 搜索仓库中的 `audit.toml` / `--ignore` 安全配置，要求项目级 `.cargo/audit.toml` 与有效 `CARGO_HOME` 下的 `audit.toml` 都不存在，确认当前三个 vulnerability 和剩余 warning 均未被静默豁免；区分 Rust crate `ignore` 与 advisory ignore 语义。

## 4. Markdown 与快照零行为回归

- [x] 4.1 运行 `cargo test --lib markdown`，确认既有默认 syntax/theme、Rust token 高亮、暗/亮主题、未知语言 fallback、宽度处理，以及 1.3 新增的未闭合 fence characterization 在最小 `syntect` features 下全部通过。
- [x] 4.2 依据 `tui-shell` delta spec 对照“默认 syntax/theme + `regex-fancy`、无 loader/UI 变化”，运行覆盖 markdown 的 `TestBackend + insta` 回归；所有既有 `.snap` 必须零 diff，禁止 approve 或改写快照基线。
- [x] 4.3 搜索全仓 `.snap.new` 并要求数量为 0；若出现任何新快照或现有快照 churn，按 feature 收窄回归处理并修正实现，不能以本 change 不改视觉为由接受差异。

## 5. 独立 security-audit workflow

- [x] 5.1 新增 `.github/workflows/security-audit.yml`：单个 `ubuntu-latest` job、`permissions: contents: read`、`timeout-minutes: 15`，每个 `pull_request`、每次 `push` 到 `master`、每周 schedule 和 `workflow_dispatch` 都可触发；不使用 paths filter，不得修改现有 `ci.yml` 的触发条件或双平台 matrix。
- [x] 5.2 将 checkout 固定为完整 commit SHA（规划基线 `08eba0b27e820071cde6df949e0beb9ba4906955 # v4.3.0`）并设置 `persist-credentials: false`；Bash preflight 以 `test -f Cargo.lock`、`test ! -L Cargo.lock`、`git ls-files --error-unmatch -- Cargo.lock` 及 mode=`100644` 断言根 lockfile 是已跟踪 regular non-symlink file，并拒绝项目根 `.cargo/audit.toml`。
- [x] 5.3 设置 `CARGO_AUDIT_VERSION=0.22.2`、`CARGO_HOME="$RUNNER_TEMP/cargo-audit-home"`、`AUDIT_ROOT="$RUNNER_TEMP/cargo-audit-root"`，要求隔离目录初始不存在且不含 `audit.toml` / `config.toml`；从 `$RUNNER_TEMP` 运行 `cargo install cargo-audit --version "$CARGO_AUDIT_VERSION" --locked --root "$AUDIT_ROOT"`。
- [x] 5.4 定义 `AUDIT_BIN="$AUDIT_ROOT/bin/cargo-audit"`，以 `test "$("$AUDIT_BIN" --version)" = "cargo-audit $CARGO_AUDIT_VERSION"` 断言完整版本输出，并执行 `"$AUDIT_BIN" audit --file "$GITHUB_WORKSPACE/Cargo.lock"`；workflow 中 MUST NOT 出现 `cargo audit` external-subcommand dispatch。
- [x] 5.5 静态审查 workflow：不含 `continue-on-error`、warning deny、advisory ignore、project `target` cache 或 advisory database cache；安装、绝对 binary 版本断言、advisory database fetch/load、指定 lockfile 读取/解析与 vulnerability 非零退出均直接使 job 失败；crates.io index/yanked 明确为 best-effort。
- [x] 5.6 检查所有 PR、master push、schedule 与 `workflow_dispatch` 都运行同一个 security job；在隔离临时副本验证 missing/untracked/symlink/mode 异常 `Cargo.lock`、项目 `.cargo/audit.toml`、隔离 home 初始配置、版本不匹配均 fail-closed，并加入恶意 `.cargo/config.toml [alias] audit` case，证明绝对 binary 仍执行真实审计。不得读取或修改用户真实 `CARGO_HOME`。

## 6. 贡献者与发布文档

- [x] 6.1 更新 `CONTRIBUTING.md` 的提交前质量基线：加入直接 `cargo-audit audit --file Cargo.lock`，并让现有构建/测试示例与 `--locked`、release build 事实一致；说明 `security-audit.yml` 是独立 required-check 候选。
- [x] 6.2 更新 README 工程质量段，区分 Windows + Ubuntu `ci.yml` 与单 Ubuntu `security-audit.yml`，说明 vulnerability hard-fail、informational warning report-only；security badge 可选，不作为验收要求。
- [x] 6.3 在 `CHANGELOG.md` Unreleased 记录 3 个 vulnerability 修复、`syntect` loader 依赖面收窄和新增 RustSec gate，不宣称 warning-free。

## 7. 全量质量门禁与范围审查

- [x] 7.1 运行 `cargo fmt --all -- --check` 与 `cargo clippy --all-targets --locked -- -D warnings`，要求全部通过。
- [x] 7.2 运行 `cargo test --locked`，要求全量 lib / integration tests 通过，并记录 ignored test 数量；不得只用定向测试代替全量验证。
- [x] 7.3 运行 `cargo build --release --locked`，要求 release build 通过。
- [x] 7.4 再次直接运行 `cargo-audit audit --file Cargo.lock`，要求退出码 0、没有 vulnerability 条目且剩余 warning 可见；随后运行 `openspec validate harden-dependency-security --strict` 与 `openspec validate --all --strict`。
- [x] 7.5 运行 `git diff --check`、`git diff --name-only`、`git ls-files --others --exclude-standard` 与 `git status --short`，并对未跟踪新文件扫描行尾空白；确认范围只含 `Cargo.toml`、`Cargo.lock`、`security-audit.yml`、`src/tui/markdown.rs` test-only 变更、`CONTRIBUTING.md`、`README.md`、`CHANGELOG.md` 和本 change OpenSpec artifacts。既有 `.snap`、`src/` 其他文件、现有 `ci.yml` 与无关文件不得变化。

## 8. Local apply 交付

- [x] 8.1 核对 `manual-verification.md` 的命令、warning 预期与 job 名称和最终实现一致，但不写入实际结果、不代勾 10.x 用户项。
- [x] 8.2 向用户汇报 3 个 vulnerability 的修复方式、已移除依赖、仍保留的 warning、完整自动验证结果及零快照 churn；到此只可表述为“local apply ready”，然后停下交给用户执行 §10。

## 10. 验收 / 远端门禁（用户可亲测；经用户显式授权可由实施 agent 完成）

- [x] 10.1 按 `manual-verification.md` 完成本地 RustSec 验证；用户亲测或经用户显式委托的实施 agent 均可依据真实输出勾选，通过前不得提交/推送。
- [x] 10.2 按 `manual-verification.md` 完成 locked 依赖树验证；用户亲测或经用户显式委托的实施 agent 均可依据真实输出勾选，通过前不得提交/推送。
- [x] 10.3 运行已通过 7.3 的 release binary 完成 markdown 真机冒烟；用户亲测或经用户显式委托且实际观察 TUI 的实施 agent 均可勾选，通过前不得提交/推送。
- [x] 10.4 PR 上独立 `security-audit` job 首次 green；通过前不得 merge。
- [x] 10.5 合入后在 `master` 上 `workflow_dispatch` 仍 green；通过前不得 archive。
