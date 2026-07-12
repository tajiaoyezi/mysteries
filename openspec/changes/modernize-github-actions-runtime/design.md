## Context

仓库当前只有 `.github/workflows/ci.yml` 与 `.github/workflows/security-audit.yml` 两个 workflow。最新 `master` 的 Windows、Ubuntu 与 RustSec jobs 均成功，但日志显示 `actions/checkout@v4`、`actions/cache@v4` 和安全审计固定的 checkout `v4.3.0` 面向 Node.js 20，已由 GitHub-hosted runner 强制转到 Node.js 24 compatibility fallback；cache 还产生 `punycode` 与 `url.parse()` deprecation warning。

普通 CI 目前以可移动的 `@v4` 引用 checkout/cache，未声明 workflow 权限，checkout 也保留默认的凭据持久化行为。安全审计已有 `contents: read`、完整 SHA 与 `persist-credentials: false`，但其 checkout runtime 同样过期。后续 release automation 会增加更高权限的发布边界，因此应先把现有只读 CI 基线收口。

本 change 不属于 headless 内核，也不触及 TUI；不走 Rust RED→GREEN 或快照流程。主要证据来自 workflow 静态约束、PR/`master` Actions 结果及迁移前后日志对比。

## Goals / Non-Goals

**Goals:**

- 让现有两个 workflow 使用官方 Node.js 24-compatible Action release，不依赖 runner 对 Node.js 20 的强制兼容。
- 将所有现有外部 `uses:` 固定到官方 tag 所指向的完整 commit SHA，并保留邻近 release tag 注释。
- 将现有 workflow 收敛到 `contents: read`，所有 checkout 均不持久化凭据。
- 保持双平台 CI、cache、RustSec audit 的触发、命令、失败与报告语义不变。
- 为后续 release automation 提供可复用但不预先扩大权限的安全基线。

**Non-Goals:**

- 不新增 release workflow，不创建 artifact、checksum、tag 或 GitHub Release。
- 不配置 GitHub branch/tag ruleset，不改变 required check 名称或 merge 策略。
- 不修改 `Cargo.toml`、`Cargo.lock`、`rust-toolchain.toml`、MSRV、`cargo-audit` 版本或 advisory policy。
- 不修改 Rust 源码、测试、快照、README 归档计数或 `v1.2.0` 版本元数据。
- 不使用 `ACTIONS_ALLOW_USE_UNSECURE_NODE_VERSION` 掩盖 runtime 过期，也不承诺 self-hosted runner 兼容。

## Decisions

### 1. 使用当前官方 Node.js 24 release，并固定 tag 对应完整 SHA

proposal 时从官方 repository 重新核对 tag ref 与 `action.yml`：

- `actions/checkout v7.0.0` → `9c091bb21b7c1c1d1991bb908d89e4e9dddfe3e0`，`runs.using = node24`。
- `actions/cache v6.1.0` → `55cc8345863c7cc4c66a329aec7e433d2d1c52a9`，`runs.using = node24`。

workflow 使用 `<owner>/<repo>@<40-hex-sha> # <tag>`，不使用 `@v7`、`@v6` 或 branch。实现前再次查询官方 tag ref；若映射变化则停止并调查，不静默采用新 SHA。

**备选：**继续使用 v4 并依赖 runner fallback。弃用，因为 warning 已证明 runtime 生命周期结束，且会把技术债复制到 release workflow。

**备选：**仅升级到可移动 major tag。弃用，因为现有 `dependency-security` 已要求安全审计使用不可变 Action 引用，普通 CI 不应采用更弱的供应链边界。

### 2. 两个现有 workflow 统一只读权限与无持久凭据 checkout

在 workflow 顶层设置 `permissions: contents: read`；所有 checkout 显式配置 `persist-credentials: false`。当前 jobs 只需读取 repository、使用 cache service 并执行本地 Cargo 命令，无需 contents write、PR write、OIDC 或 package 权限。

**备选：**依赖 repository 默认 workflow permission。弃用，因为外部设置可漂移，workflow 应自带最小权限事实源。

**备选：**保留 checkout 默认凭据。弃用，因为后续 Cargo/test steps 不需要 Git push，持久化 token 只扩大被脚本读取或滥用的边界。

### 3. Action 边界之外只增加确定性的 tested-revision evidence step

两个 workflow 均在 checkout 后增加同名 `Show tested revision` step，只执行 `echo "TESTED_REVISION=$(git rev-parse HEAD)"`。固定前缀让证据脚本不依赖 `gh run view` 的 step-name 列或 checkout action 内部 debug 文本，同时区分 Actions REST `run.head_sha`（PR head）与 runner 实际 checkout 的 synthetic merge commit。该 step 只读、不新增 Action、不请求权限、不改变后续命令。

除此之外，CI 的 `push master` / `pull_request`、job/check 名称、Windows/Linux matrix、cache paths/key、fmt、clippy、full test 与 release build 命令保持逐项一致。Security audit 的 job/check 名称、triggers、schedule、`workflow_dispatch`、15 分钟 timeout、input validator、隔离安装、完整版本断言、绝对 binary path 与 `--deny unsound` 保持一致。

同仓 implementation PR 首次升级后的 cache miss 可接受，但 cache key 和 path 不变；具备 read-write cache token 时后续 restore/save 必须正常完成，外部 fork 的合法 read-only save denial 例外由 §4 处理。既有 `bincode` unmaintained warning继续可见且不阻断，仍以 exit 0 证明 0 vulnerability / 0 unsound，不宣称 warning-free。

**备选：**顺手优化 cache key、增加 concurrency 或升级 `cargo-audit`。弃用，因为会扩大 diff、混淆 runtime 迁移的回归归因。

### 4. 用不可变 commit 的外部 Actions 日志闭环，不让 commit 证明自己

`pull_request` workflow 默认测试 `refs/pull/<number>/merge` 的 synthetic merge commit，而不是孤立的 PR head。Actions REST `run.head_sha` SHALL 只用来绑定 PR head，MUST NOT 当作 runner 的 `GITHUB_SHA`。最终 implementation evidence MUST 同时固定 PR head SHA、当时的 base SHA、PR API 在 run 时的 `merge_commit_sha`、三个 jobs 的 `Show tested revision` 输出、run ID 与 run attempt；两个 workflow 的 `run.head_sha` MUST 等于 PR head，三个 jobs 的 revision 输出 MUST 等于同一 tested merge-ref SHA。还 MUST 查询该 merge commit object，断言其 first parent 为记录的 base、second parent 为记录的 head。若 head 或 base 漂移，旧 run 只证明旧二元组，必须等待新 merge-ref runs，MUST NOT 把旧绿灯描述为新 head 的验证。

GitHub 在 PR 合入后会把 REST `merge_commit_sha` 改为实际 implementation merge commit，不再返回合入前的 synthetic merge-ref。证据模板在 PR open 时 MUST 将 live 字段与记录值比较；在 merged 状态重放时 MUST 使用已经持久化的 `PR_API_MERGE_SHA` / `TESTED_MERGE_SHA` 作为 immutable expected value，并继续用 job markers 与 merge parents 验证，不得把合入后的 live 字段覆盖到历史证据。

若 durable evidence PR 合入后的审查发现 OpenSpec artifact 缺陷，允许创建新的 bounded review-remediation evidence carrier。该 carrier 只能修改 change artifacts，必须在 commit 前记录新 branch 并保留被取代 branch，不得写入自身尚未生成的 SHA/PR number；合入后由 archive gate 验证新 carrier 的精确 merge SHA。这样可以修复证据脚本而不把未提交修改静默带入 archive，也不要求 commit 证明自身。

最终 merge-ref runs 必须通过 Windows、Ubuntu 与 Security audit；日志不得再出现 Node.js 20 compatibility fallback、`DEP0040 punycode` 或 `DEP0169 url.parse()`。用于 cache save/restore 证明的 implementation PR MUST 来自同一 repository 分支；若其 cache 首轮 miss，必须确认 post-job save 成功，再 rerun 同一 workflow run（相同 tested merge-ref SHA、递增 run attempt），证明相同 OS/lockfile key 可正常 restore；若首轮已 hit，则保存 restore 证据，无需为了制造 miss 清除共享 cache。外部 fork PR 若由 GitHub 发放 read-only cache token，允许 save-denied warning 且 job 继续成功；这不属于本 change 要消除的 runtime/cache deprecation warning，也不得改用 `pull_request_target` 获取写权限。

implementation PR 合入后，必须检查其精确 implementation merge commit 对应的 `master` runs，而不是默认分支的任意较新成功结果。迁移前基线、official tag/SHA、PR revision tuple、run IDs/attempts、cache 与 implementation merge runs MUST 持久写入 change 内 `manual-verification.md`；`tasks.md` 仍是唯一进度状态源。随后由独立 post-merge evidence commit/PR 提交该证据与已完成 task 状态；这个 evidence commit 只证明更早的 implementation revisions，不得宣称其自己的 checks 已被它自己记录。

evidence PR 合入后，其精确 merge commit 对应的最终 `master` checks 仍是 archive 外部门禁。archive 前必须按 `manual-verification.md` 的命令查询这些 runs，并把 merge SHA、run IDs/attempts 与结论写入经用户审阅的 archive 决策记录；不得为了把该最终结果写回 `manual-verification.md` 再追加递归 evidence commit。

本地/静态验证负责确认 YAML scope、完整 SHA、tag 注释、权限、凭据和未变化的命令；GitHub-hosted runtime 与 cache 行为只能由真实 Actions 日志最终证明。

## Risks / Trade-offs

- [官方 tag 或 SHA 抄录错误] → 实现前查询官方 tag ref，并将完整 SHA 与 release tag 注释一起审查。
- [checkout/cache 新 major 行为变化] → 保持所有 inputs 不变，在 Windows/Linux 与 Security audit 三个真实 jobs 上验证；失败则不合入。
- [升级后首次 cache miss 拉长 CI] → 接受一次冷启动，不改变 cache key/path；后续 run 验证 restore/save 正常。
- [只看 warning 消失而漏掉功能回归] → 同时要求现有 fmt、clippy、全量测试、release build 和 RustSec gate 全绿。
- [固定 SHA 随时间陈旧] → 本 change 保证不可变与当前受支持 runtime，不承诺自动追新；后续升级仍走独立 change。
- [未来 release job 需要写权限] → 当前统一基线保持只读；未来仅在独立 publish job 中按 spec 精确授予 `contents: write`。

## Migration Plan

1. 保存当前 `master` Actions annotation 作为迁移前证据，并再次核对官方 tag → commit SHA 与 `runs.using = node24`。
2. 先修改普通 CI 的 checkout/cache、权限与 checkout credentials，再修改安全审计 checkout；两个 checkout 后各增加同名 `Show tested revision` step，不调整其他 YAML 字段。
3. 运行静态 scope/不变量检查与 OpenSpec strict validation，确认没有 Rust/Cargo diff。
4. 从同一 repository 分支推送 implementation PR，记录 head/base、PR API merge SHA、三个 `Show tested revision` 输出与 merge parents；等待 Windows、Ubuntu 与 Security audit，审查完整日志中的 runtime/cache warning，cache miss 时按 attempt 保存首轮日志并 rerun 同一 run/merge-ref验证 restore。
5. 合入 implementation PR，检查精确 implementation merge commit 的 `master` CI 与 Security audit，并把 revision tuple、run IDs/attempts、cache 与 merge evidence 填入 `manual-verification.md`。
6. 以独立 post-merge evidence commit/PR 提交 durable evidence 并完成远端 tasks；该 commit 只证明更早的 implementation revisions。
7. evidence PR 合入后，archive 前查询其精确 merge commit 的最终 `master` runs，并把结果写入用户批准的 archive 决策记录；不再修改 change evidence 制造循环。

若新 major 在 GitHub-hosted runner 上产生不兼容，PR 不合入并修正设计；若问题只在合入后出现，则 revert 本 change 的 workflow commit，恢复上一已知可运行版本，同时保留弃用 warning 作为待修问题，不通过移动 tag 或禁用检查规避。

## Open Questions

- 无。本 change 的 Action release、权限边界和行为保持策略已收敛；branch/tag protection、MSRV 与 release workflow 均留给后续 change。
