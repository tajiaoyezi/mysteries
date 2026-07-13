## ADDED Requirements

### Requirement: Release workflow 将写权限隔离在 tag publish job

`.github/workflows/release.yml` SHALL 默认仅授予 `contents: read`。Windows/Linux version/package/build、artifact upload、download verification 与公开后 smoke jobs MUST 保持只读；只有同时满足 `push` event、canonical stable SemVer annotated tag、版本一致性与全部 build dependencies 成功的 publish job MAY 在 job level 授予唯一 `contents: write`，并 MUST NOT 获得 `id-token`、`attestations`、`packages`、`pull-requests`、`actions`、`checks`、`issues` 或其他写权限。非publish checkout MAY 仅通过官方`actions/checkout`默认token input使用内建只读`github.token`，但 MUST 设置`persist-credentials: false`；除此之外，validation/build/package/checksum/public smoke steps MUST NOT 把`github.token`显式传作Action input，也不得设置`GH_TOKEN`/`GITHUB_TOKEN`环境变量。publish job MUST NOT checkout或执行仓库代码；`GH_TOKEN=${{ github.token }}` MUST 只绑定到实际调用GitHub Release API的shell steps，不得传入artifact download、checksum或其他非API steps。公开后smoke MUST 使用匿名HTTPS asset URL，不得调用需要认证的GitHub CLI/API。

release workflow 的每个第三方 Action MUST 固定到经官方 release tag 映射核验的完整 40 位 commit SHA，并在邻近注释可读 tag；JavaScript Action MUST 使用 GitHub-hosted runner 当前受支持的 runtime，不得依赖 compatibility fallback或 `ACTIONS_ALLOW_USE_UNSECURE_NODE_VERSION`。每次 checkout MUST 设置 `persist-credentials: false`。workflow MUST NOT 使用第三方 release Action、`pull_request_target`、`continue-on-error`、clobber/overwrite、用户 PAT 或长期 credential；未知/不合法 ref、版本不一致、artifact/checksum/API 验证错误均 MUST fail-closed。现有 `.github/workflows/ci.yml` 与 `security-audit.yml` 的只读权限、触发与 job 契约 MUST 保持不变。

#### Scenario: PR 与 dry-run 没有 release 写权限
- **WHEN** release workflow 由 `pull_request` 或 `workflow_dispatch` 触发
- **THEN** 所有 jobs 的 token 权限最多为 `contents: read`，publish job 不运行，任何 step 都不能创建 tag、Release 或 asset

#### Scenario: 合法 tag 只给 publish job contents write
- **WHEN** 严格 stable SemVer tag 的版本与 artifacts 已验证且 publish job 开始
- **THEN** 仅该job获得`contents: write`，publish job不checkout，`GH_TOKEN`环境变量只绑定draft/create/upload/API verify/publish shell steps；build与公开后smoke jobs仍为只读，后者通过匿名HTTPS下载且没有token环境变量

#### Scenario: 发布 workflow 的 Action 全部不可变且 runtime 受支持
- **WHEN** 静态审查 `release.yml` 的每个 `uses:`
- **THEN** 每个引用均为官方 tag 对应的完整 commit SHA并有邻近 tag 注释，JavaScript runtime 受支持，不存在 floating ref或不安全 runtime override

#### Scenario: Checkout credential 不进入构建环境
- **WHEN** release workflow 在任一事件和平台执行 checkout
- **THEN** 只有官方checkout step可通过默认input使用内建只读token，`persist-credentials: false`生效，后续Cargo、package、checksum与smoke steps的environment及repository config中均没有checkout token

#### Scenario: Publish job 不把写 token 交给 checkout 或仓库代码
- **WHEN** 合法tag进入具有`contents: write`的publish job
- **THEN** 该job不执行checkout、Cargo、仓库脚本或release binary，且`GH_TOKEN`只存在于Release API shell steps

#### Scenario: 公开后 smoke 不使用认证
- **WHEN** Windows/Linux public smoke jobs验证已公开Release
- **THEN** jobs仅通过匿名HTTPS asset URL下载，不设置`GH_TOKEN`/`GITHUB_TOKEN`环境变量且不调用GitHub CLI/API

#### Scenario: 非法发布输入 fail-closed
- **WHEN** ref/event/version不合法、artifact或checksum异常、GitHub API返回不一致、已有同 tag Release，或任一 publish命令失败
- **THEN** workflow 以失败状态结束且不得公开部分 Release、覆盖既有 asset或扩大权限重试

#### Scenario: 现有 CI 与 Security 权限不被发布能力扩张
- **WHEN** 对比本 change 前后的 `ci.yml` 与 `security-audit.yml`
- **THEN** 二者触发条件、job/check名称、`contents: read`、checkout credential、测试和RustSec语义保持不变
