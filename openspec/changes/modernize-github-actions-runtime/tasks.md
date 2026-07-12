## 1. 固定迁移基线

- [x] 1.1 读取最新 `master` 的 CI 与 Security audit runs，在 `manual-verification.md` 记录 baseline master SHA、run IDs/attempts、Node.js 20 compatibility fallback、`DEP0040 punycode` 与 `DEP0169 url.parse()` 的迁移前日志证据，并确认三个 jobs 当前均成功而非以失败掩盖 warning
- [x] 1.2 从 `actions/checkout` 与 `actions/cache` 官方 repository 重新核验目标 release tag、tag ref 完整 commit SHA 及 `action.yml` 的 `runs.using = node24`；映射必须分别仍为 design 中记录的 checkout `v7.0.0` / cache `v6.1.0`，不一致时停止并更新设计而非静默换值
- [x] 1.3 在 `manual-verification.md` 逐项记录两个现有 workflow 的 triggers、job/check 名称（含 `RustSec dependency audit`）、Windows/Linux matrix、cache paths/key、Cargo 命令及 RustSec audit policy，作为迁移后不变量清单

## 2. 迁移现有 GitHub Actions

- [x] 2.1 修改 `.github/workflows/ci.yml`：将 checkout 与 cache 替换为官方 tag 对应的完整 40 位 SHA并添加邻近 release tag 注释，新增 workflow 级 `permissions: contents: read`，为 checkout 显式设置 `persist-credentials: false`；checkout 后增加 `Show tested revision` step，仅执行 `echo "TESTED_REVISION=$(git rev-parse HEAD)"`
- [x] 2.2 修改 `.github/workflows/security-audit.yml`：仅把 checkout 替换为与普通 CI 相同的 Node.js 24-compatible 完整 SHA及 tag 注释，继续显式设置 `persist-credentials: false`；checkout 后增加与 CI 同名同命令的 `Show tested revision` step
- [x] 2.3 确认未设置 `ACTIONS_ALLOW_USE_UNSECURE_NODE_VERSION` 或等价规避开关，未新增其他 Action、权限、secret、dependency、workflow 或 job

## 3. 静态契约验证

- [x] 3.1 扫描 `.github/workflows/**/*.yml` 的全部外部 `uses:`，断言每项均使用完整 40 位 SHA并有邻近 release tag 注释，不存在 `@vN`、branch 或 floating tag
- [x] 3.2 对照 1.3 不变量清单，确认普通 CI 除新增只读 revision step 外，triggers、check 名称、matrix、cache paths/key、fmt、clippy、全量 test 与 release build 命令逐项未变
- [x] 3.3 确认 Security audit 除新增只读 revision step 外，`RustSec dependency audit` job/check 名称、triggers、schedule、`workflow_dispatch`、timeout、固定 `cargo-audit` 版本、输入校验、隔离安装、绝对 binary path、`--deny unsound` 与 fail-closed 逻辑逐项未变
- [x] 3.4 确认两个 workflow 均仅声明 `contents: read`，每个 checkout 均设置 `persist-credentials: false`，不存在 contents/PR/OIDC/package write 权限

## 4. 本地回归与范围审计

- [x] 4.1 运行 `cargo fmt --all -- --check` 与 `cargo clippy --all-targets --locked -- -D warnings`
- [x] 4.2 运行 `cargo test --locked` 与 `cargo build --release --locked`
- [x] 4.3 按现有 Security audit 的隔离边界运行固定版本 `cargo-audit`，确认 `audit --deny unsound --file <absolute-root-Cargo.lock>` 为 0 vulnerability / 0 unsound，且既有 `bincode` unmaintained warning 仍可见、不被写成 warning-free
- [x] 4.4 运行 `openspec validate --all --strict` 与 `git diff --check`，确认规划/实现文件通过严格校验
- [x] 4.5 审查最终 diff：除本 change 的 OpenSpec artifacts、task 状态与 `manual-verification.md` 证据外，实现文件侧只允许两个 workflow 改动，不得出现 Rust/Cargo/toolchain/snapshot/release/version/README 或 GitHub ruleset 相关变更；输出权限、凭据、Action SHA 与行为不变量核对结果

## 5. 远端运行与不可变证据

- [ ] 5.1 在用户授权 Git 写操作后从同一 repository 分支创建 implementation PR；在 `manual-verification.md` 锁定 PR head/base、PR API merge SHA、三个 jobs 的 `Show tested revision` 输出、run IDs/attempts，断言两个 workflow 的 REST `run.head_sha` 等于 PR head、三个 revision 输出等于同一 tested merge-ref SHA，且该 merge commit 的 first/second parents 分别等于 base/head；若任一 revision 漂移则废弃旧证据并等待新 runs
- [ ] 5.2 等待 5.1 所记录 merge-ref SHA 的 Windows CI、Ubuntu CI 与 RustSec dependency audit 全部成功，逐 job 检查完整日志/annotations 不含 Node.js 20 deprecated、被强制运行于 Node.js 24、`DEP0040 punycode` 或 `DEP0169 url.parse()`；检查两个平台 cache，首轮 hit 时记录 restore，首轮 miss 时保存该 attempt 的 post-job save 日志并 rerun 同一 run（相同 merge-ref SHA、递增 attempt），再以 `--attempt` 读取新日志验证 restore，不得改 key、清 cache 或改变 revision 制造结果
- [ ] 5.3 仅在 5.1/5.2 成立后合入 implementation PR；在 `manual-verification.md` 记录精确 implementation merge SHA，按 workflow + `event=push` 唯一查询该 SHA 的 CI 与 Security audit runs，验证 Windows/Ubuntu/Security 全部成功且同样无四类 runtime/cache deprecation 语义，记录 run IDs/attempts
- [ ] 5.4 创建独立 post-merge evidence branch，先把 branch name 写入 `manual-verification.md`，再以一个原子 evidence commit 提交已填充的手册与 5.1-5.4 完成状态；勾选 5.4 仅表示该 branch 与 durable evidence commit 已创建，文件不得自引用尚未生成的 commit SHA/PR number，该 commit 只能证明更早的 implementation revisions

> 非 checkbox archive precondition：5.4 的 evidence commit 创建后，仍须 push、创建并合入 evidence PR；其精确 evidence merge SHA 的最终 `master` push checks 必须由 archive 阶段按 `manual-verification.md` 查询，并写入用户审阅的 archive 决策记录。不得让 evidence commit 证明自身，也不得追加递归 evidence commit。
