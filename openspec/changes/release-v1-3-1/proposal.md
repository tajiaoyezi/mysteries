## Why

`v1.3.0` tag run 在成功创建 draft Release 并上传三个 sealed assets 后，立即从 paginated Release 列表按 tag 读取 ID，唯一匹配断言失败；随后同一唯一 draft 可见。该现场与短暂 eventual-consistency 不可见窗口相容，但失败日志没有记录当时的match count，不能据此确认根因。现有规则要求保留失败 run、tag 和非公开 draft，禁止 rerun 或复用 `v1.3.0`，因此必须以新的 `v1.3.1` patch release移除create后的不必要列表发现窗口并重新走完整发布门禁。

## What Changes

- 永久保留 `v1.3.0` annotated tag、失败 run `30023496746`、deployment approval、draft Release `358808315` 及其三个 assets，不删除、不移动、不公开、不 rerun。
- 直接通过官方 Create Release REST API 创建 draft，并从同一 `201` 响应捕获 numeric Release ID 与 `upload_url`；上传、metadata/正文/assets identity 验证及 public PATCH 全程绑定该 ID，不再通过 paginated Release 列表重新发现刚创建的对象。
- 为 create 响应缺失/非法 ID 或 `upload_url`、错误 tag/draft/body identity、asset upload 响应漂移、既有 Release/POST 冲突，以及“列表持续不可见但 captured ID 仍可读取”的路径增加无真实 credential 的静态/fixture 回归验证。
- 将根 package/lockfile、Changelog、README、release notes、版本敏感 snapshots 与 release assets 迁移到 `1.3.1`；不新增 Rust runtime 行为、dependency、config/session wire 或 subagent 能力。
- 在 Changelog 中把 `v1.3.0` 明确记录为保留 tag/draft 但未公开且版本已消耗；`v1.3.1` 是 v1.3 功能集的首次公开交付并包含本次 post-create list rediscovery failure window 修复。
- 在 tag 前把 `release` environment 的唯一 custom tag policy 从 `v1.3.0` 切换为精确 `v1.3.1` 并重新读取验证；immutable setting、reviewer/self-review/admin-bypass 与两组 rulesets 保持不变。
- 重新执行 implementation PR、master CI/Security/dry-run、annotated `v1.3.1` tag、protected environment approval、immutable public Release、匿名双平台 smoke 与 Windows TUI 真机门禁。

## Capabilities

### New Capabilities

无。

### Modified Capabilities

- `release-delivery`: 明确 draft 创建必须从官方 REST `201` 响应捕获唯一 Release ID 与 upload endpoint，后续上传、验证和公开均绑定该 ID而不依赖列表读后可见；同时固化 `v1.3.0` 失败对象不可复用及 `v1.3.1` patch release 的完整证据链。

## Impact

- 主要影响 `.github/workflows/release.yml`、根版本/发布文档、版本敏感 TUI snapshots，以及 `release-v1-3-0` / `release-v1-3-1` OpenSpec 证据；`deliverables/README.md` 保持零 diff。
- 不修改 Rust runtime source、CI/Security workflow、dependency graph、公开 CLI/API、session/config schema 或 TUI 交互；允许新增只约束 release workflow 的 Rust integration test。
- GitHub 远端新增的正式对象只能是新的 annotated `v1.3.1` tag 与其 Release；现有 `v1.3.0` 失败对象保持原状。
