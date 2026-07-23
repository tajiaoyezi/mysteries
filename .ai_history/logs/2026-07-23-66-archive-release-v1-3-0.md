# 2026-07-23 · 66 · archive-release-v1-3-0

## 决策
- `v1.3.0` 以 `terminated-by-failure` 而非成功发布状态归档 | 选:保留annotated tag、attempt 1失败run、deployment approval、非公开draft及三个assets原状，并让成功publish/public/smoke tasks保持未勾 | 弃:rerun同tag（违反version消耗契约）、删除或重建draft/tag（破坏失败证据）、人工公开残留draft（绕过修复与完整门禁） | 主导:讨论收敛 | 依据:Actions run、Release API、sealed bundle identity、OpenSpec
- 修复使用独立`v1.3.1` patch change，并从Create Release `201`响应捕获authoritative Release ID与upload URL | 选:后续upload/GET/PATCH全程绑定captured identity | 弃:paginated list bounded retry（仍依赖未承诺的读后可见时限）、HTML URL解析（不是numeric API identity）、继续使用`gh release upload`（再次按tag发现对象） | 主导:讨论收敛 | 依据:workflow失败step、官方Release REST contract、对抗式审查
- 归档只声明旧change已按失败边界收口，不声明`v1.3.0`已公开或验收成功 | 选:同步已实现的通用release/security contract并增加failure-termination证据例外 | 弃:为消除warning伪勾成功任务、把非公开draft写成latest Release | 主导:Agent | 依据:code、tasks、remote state

## 证据
- candidate与tag:`c06cf3b4ecb006003e453beeb6fa0b3f0eb05fc0`;annotated tag object=`d4df2273298591bbb8657ce0d36e81206d08e4c2`;远端`v1.3.0^{}`peeled SHA等于candidate。
- Actions:tag run=`30023496746`,attempt=`1`,head SHA等于candidate,conclusion=`failure`;publish job=`89263361613`;失败step=`Fetch draft Release metadata through API`。
- Release:draft ID=`358808315`,tag=`v1.3.0`,target观察值=`master`,draft=`true`,prerelease=`false`,immutable=`false`;body=`1580` bytes,SHA-256=`835b4d5255f16cd013cc1433ed9c6467f8a26089f4bcc2aa5e8b03ae032e2417`，与sealed release notes逐字节相等。
- Windows asset:id=`487376022`,name=`mysteries-v1.3.0-x86_64-pc-windows-msvc.zip`,size=`4245061`,SHA-256=`937961b8acda2351df3c1bda717f5431a4048ecb2efec889d2a84401d0f6d19b`。
- Linux asset:id=`487376021`,name=`mysteries-v1.3.0-x86_64-unknown-linux-gnu.tar.gz`,size=`4970546`,SHA-256=`6211b2d3b13a56a935204bb745477634cd7e9f78f0a8eb4a5fc8ca63e36d5bb1`。
- Manifest asset:id=`487376020`,name=`SHA256SUMS`,size=`225`,SHA-256=`5aae3e563c9f88314206d2e16a2e22a08e26440c913eaa5022ec065bcf2bde9b`;三个remote assets均与tag run sealed bundle逐字节相等。

## 变更
- 固化release event/attempt、protected environment、rulesets、Actions evidence、candidate ancestry、sealed asset/body identity、最小token scope及失败后version不可复用契约。
- 记录candidate、tag、run、draft与asset identity；成功publish、public downloads、Windows TUI与成功archive链保持未完成。
- 建立`release-v1-3-1`，用于移除post-create list rediscovery failure window并重新执行完整发布门禁。

## 待决
- 无；`v1.3.0`失败对象永久保留，公开latest在`v1.3.1`成功前仍为`v1.2.0`。

## 引用
- OpenSpec change: `release-v1-3-0`
- Follow-up OpenSpec change: `release-v1-3-1`
