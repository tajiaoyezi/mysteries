# 2026-07-21 · 65 · archive-add-readonly-subagent

## 决策
- v1.3首个可用subagent收敛为单层临时只读委派 | 选:`delegate_task`只向child暴露`list_dir`、`read_file`、`glob`、`grep`，以restricted registry、execution scope与depth 0 child共同fail-closed | 弃:递归Agent graph（缺少全局预算）、可写/联网/执行child（扩大权限与审计面）、第三方Agent SDK（违反核心能力自研边界） | 主导:讨论收敛 | 依据:code/tests/spec
- 并发复用既有Agent安全批次 | 选:`delegate_task`标记为`ReadOnly + ParallelSafe`，最多4个outer child同时active，第5项等待后继续，结果按模型occurrence发布 | 弃:新增第二套child scheduler（重复取消、屏障与顺序语义）、把4误作单轮总调用上限（与已批准行为不符） | 主导:讨论收敛 | 依据:Barrier tests/manual verification
- Provider/model与终止语义以invocation snapshot和publication checkpoint收口 | 选:pair switch原子提交、运行中child冻结调用时tuple；串行及parallel ready buffer在紧邻发布前复查parent termination | 弃:assembly时捕获runtime（产生陈旧child）、分别切换Provider/model（可能撕裂）、把ready buffer视作已发布（可能泄漏取消后的普通结果） | 主导:对抗式审查收敛 | 依据:RED→GREEN tests/spec
- workspace与UI/session均坚持最小暴露 | 选:canonical read root覆盖absolute、`..`、symlink/junction及ignore控制文件；TUI只复用outer C5卡、过滤child status/tool事件但累计usage，session只保存outer occurrence | 弃:仅靠system prompt限制路径（可绕过）、新增subagent面板或child session（超出MVP） | 主导:用户真机验证与讨论收敛 | 依据:security tests/snapshots/manual verification

## 变更
- 新增单层只读`delegate_task`、共享`AgentRuntime` snapshot、source-compatible scoped Tool seam、受限child registry及child-only workspace containment。
- TUI与headless产品root开放一层child depth；legacy Agent wrapper保持depth 0，Provider/model picker与session restore使用原子pair replace。
- 完善parallel/post-ready cancellation、forced-final `CallingModel`/usage/`Idle` observer与outer-only session/TUI收口；新增8份Midnight/Daylight delegate卡快照并经用户批准。
- 自动化门禁及真机9.1–9.5通过，tasks为52/52；无新增dependency，config、CLI grammar、Provider wire、permission矩阵与session JSONL格式不变。
- 主spec同步6个capability：新增15项requirement、修改1项既有requirement；`builtin-tools`由12个更新为13个，新增`readonly-subagent`能力域。

## 待决
- 递归subagent、可写/联网/执行child、后台任务、child session、child专用model、token总预算、per-response delegate occurrence与child扫描字节上限留给后续change。

## 引用
- OpenSpec change: add-readonly-subagent
- Specs: agent-execution-scope、agent-loop、builtin-tools、readonly-subagent、tool-system、tui-shell
- Predecessor: add-agent-execution-scope
- Manual verification: add-readonly-subagent/manual-verification.md
- Session checkpoint: 本次未单独生成
