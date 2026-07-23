# 2026-07-23 · 66 · archive-release-v1-3-0

## 决策
- `v1.3.0` 以 `terminated-by-failure` 而非成功发布状态归档 | 选:保留annotated tag、attempt 1失败run、deployment approval、非公开draft及三个assets原状，并让成功publish/public/smoke tasks保持未勾 | 弃:rerun同tag（违反version消耗契约）、删除或重建draft/tag（破坏失败证据）、人工公开残留draft（绕过修复与完整门禁） | 主导:讨论收敛 | 依据:Actions run、Release API、sealed bundle identity、OpenSpec
- 修复使用独立`v1.3.1` patch change，并从Create Release `201`响应捕获authoritative Release ID与upload URL | 选:后续upload/GET/PATCH全程绑定captured identity | 弃:paginated list bounded retry（仍依赖未承诺的读后可见时限）、HTML URL解析（不是numeric API identity）、继续使用`gh release upload`（再次按tag发现对象） | 主导:讨论收敛 | 依据:workflow失败step、官方Release REST contract、对抗式审查
- 归档只声明旧change已按失败边界收口，不声明`v1.3.0`已公开或验收成功 | 选:同步已实现的通用release/security contract并增加failure-termination证据例外 | 弃:为消除warning伪勾成功任务、把非公开draft写成latest Release | 主导:Agent | 依据:code、tasks、remote state

## 变更
- 固化release event/attempt、protected environment、rulesets、Actions evidence、candidate ancestry、sealed asset/body identity、最小token scope及失败后version不可复用契约。
- 记录candidate、tag、run、draft与asset identity；成功publish、public downloads、Windows TUI与成功archive链保持未完成。
- 建立`release-v1-3-1`，用于移除post-create list rediscovery failure window并重新执行完整发布门禁。

## 待决
- 无；`v1.3.0`失败对象永久保留，公开latest在`v1.3.1`成功前仍为`v1.2.0`。

## 引用
- OpenSpec change: `release-v1-3-0`
- Follow-up OpenSpec change: `release-v1-3-1`
