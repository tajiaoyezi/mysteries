## Why

当前 GitHub Actions 虽然全部通过，但 `actions/checkout@v4`、`actions/cache@v4` 与已固定的 checkout `v4.3.0` 仍面向已弃用的 Node.js 20 runtime，GitHub-hosted runner 只能通过强制兼容到 Node.js 24 继续运行；普通 CI 同时仍使用可移动 major tag、默认持久化 checkout 凭据且没有显式最小权限。在新增 release workflow 前，必须先消除这些供应链与运行时债务，建立可复用的安全 CI 基线。

## What Changes

- 将两个现有 workflow 中的 `actions/checkout` 统一升级到 Node.js 24-compatible release，并固定到经官方 tag 映射确认的完整 commit SHA，邻近保留可读 release tag 注释。
- 将普通 CI 中的 `actions/cache` 升级到 Node.js 24-compatible release，同样固定完整 commit SHA并保留 release tag 注释。
- 为普通 CI 显式设置 `permissions: contents: read`，并为所有 checkout 设置 `persist-credentials: false`。
- 在两个 workflow 的 checkout 后增加同名、只读的 `Show tested revision` step，以 `echo "TESTED_REVISION=$(git rev-parse HEAD)"` 输出可稳定解析的唯一 marker，明确 runner 实际测试的 revision，不把 Actions REST `run.head_sha` 误当 synthetic merge commit。
- 把不可变 Action 引用、受支持 JavaScript runtime、最小权限与 checkout 凭据规则从安全审计 workflow 的局部约束提升为仓库现有 GitHub Actions workflow 的统一契约。
- 除新增 revision evidence step 外，保持现有触发条件、job/check 名称、Windows/Linux matrix、Cargo 校验命令、cache 路径/key、RustSec `--deny unsound`、定时审计与手动触发语义不变，并以 PR 与合入后日志证明 runtime deprecation 已消失。
- 不新增 release workflow，不修改 Rust toolchain、MSRV、`cargo-audit`、Rust 源码、Cargo dependency、版本号、artifact 或 GitHub branch/tag protection。

## Capabilities

### New Capabilities

- 无。

### Modified Capabilities

- `dependency-security`: 将不可变 Action 引用、受支持 runtime、workflow 最小权限及 checkout 不持久化凭据扩展为现有 CI 与安全审计 workflow 的统一供应链要求，同时保持依赖审计策略和双平台构建测试行为不变。

## Impact

- 受影响文件：`.github/workflows/ci.yml`、`.github/workflows/security-audit.yml` 与 `openspec/specs/dependency-security/spec.md`。
- change 内新增 `manual-verification.md`，持久记录迁移前基线、PR merge-ref、cache 与 post-merge 运行证据；它不形成第二套 task 状态。
- 外部系统：GitHub-hosted Actions runner、`actions/checkout`、`actions/cache`。
- 不影响产品 CLI/TUI、Agent Loop、权限模型、session 格式、Provider API、Cargo dependency graph 或最终用户行为。
- 该 change 是后续 release automation 与 `v1.2.0` 版本冻结的前置基础设施 change。
