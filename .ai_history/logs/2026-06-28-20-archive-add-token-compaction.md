# 2026-06-28 · 20 · archive add-token-compaction(1.1 真实上下文压缩)

## 决策
- 真实压缩(调 provider 生成结构化 summary)= claude code 式,非滑窗截断 | 主导:用户 D1 | 依据:技术方案 §13 / design
- 阈值配置驱动:model_context_window(Option,未配=禁用)× compact_trigger_ratio(默认 0.8),看 last_usage.input_tokens;无 tokenizer 用 provider 真实 usage | 主导:用户 D2 | 依据:design ②
- 压缩形态:System(原 prompt + 结构化 summary)+ 最近 keep_recent_turns 完整轮(默认 1);**summary 入 System 而非独立 message**(保 Anthropic user/assistant 交替,连续同 role→400)| 主导:用户 D3「最贴 claude code」+ 主 agent 挖出交替红线 | 依据:design ③⑤
- 二次压缩 = 重新总结(非累积):split System 的 SUMMARY_HEADER 拆旧 summary,以 [previous_summary] 纳入与新区间再压,新 System 用原 prompt → System 不膨胀 | 选:重新总结 | 弃:追加累积(长会话膨胀)| 主导:用户拍 Gap 2 + 主 agent review 发现 | 依据:design ⑤ / code
- 降级不致命:summary 失败→退回不压(Ok 原);自动失败下轮再触发、手动 /compact 可重试 | 主导:用户 D4 | 依据:design ⑥
- 手动 /compact:无视阈值复用 Compacting;降级靠 compacted==original && can_compress | 主导:用户「一起做」| 依据:design ⑦
- TUI 会话 history 跨轮累积:修既有「每 prompt 重建(多轮无记忆)」缺陷,agent_history 共享+跨轮追加+回写,压缩前置 | 主导:主 agent review 发现 + 据实补 tui-shell spec | 依据:code
- provider Box→Arc:Compacting 与 Agent 共享句柄;AssembledAgent{agent, compacting},双 Compacting 实例(同 Arc/settings)避 Box<dyn> downcast | 主导:主 agent | 依据:design ⑧
- prepare 签名加 last_usage(MODIFY context-strategy);ContextError→AgentError::Context(还掉 18 临时映射)| 依据:design ① / 决策记录 18 预告

## 变更
- 新增 src/agent/compacting.rs(Compacting + compact_history 重新总结 + run_compact_command + 测试);context.rs(prepare+last_usage);agent/mod.rs(last_usage 维护 + AgentError::Context);app.rs(AssembledAgent + provider Arc + 装配选 strategy);config(三项+校验);tui/{mod,app,channel,command}(/compact + agent_history 累积);error;e2e
- spec:context-strategy MODIFY+ADD、config-layering ADD、builtin-commands ADD、tui-shell ADD
- 验证:cargo test 210 unit + 1 e2e 全绿、clippy/fmt 净、validate --strict 过;worktree 隔离 apply + 主树 merge(ff)集成绿
- 流程:单 agent worktree apply;3 红灯停点(①prepare 签名 ②Compacting 核心 ③/compact)**全部遵守**(对比上批 A/B 跳过——本批 dispatch 写硬红灯纪律见效);review 发现 2 gap(TUI 累积无测试/spec、二次压缩累积非重压)→ 打回补齐

## 待决
- 6.4 手动冒烟未跑(配 model_context_window + 长会话自动压缩 + /compact),留用户
- model_context_window 实际值未配(deepseek-v4-pro 的 window)
- summary 调用期间无 on_status 提示(prepare 内同步阻塞;UX 小瑕疵,类似强制收尾未 emit CallingModel)
- 承前未动:git 身份 wanglei30 临时 + leafiellune purge

## 引用
- change:add-token-compaction(archive 2026-06-28-*)
- 前置:expose-token-usage + add-context-strategy(18,1.1 地基);refine-tool-card-folding(19)
- session:本会话——1.1 规划(D1-D4)+ 单 agent worktree apply + 3 红灯停审 + review 2 gap 打回
- memory:subagents-skip-red-light-stops(本批 dispatch 写硬→守纪律,验证该条 how-to-apply)
