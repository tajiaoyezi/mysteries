# 2026-06-28 · 18 · archive expose-token-usage + add-context-strategy(1.1 地基·并行 apply)

## 决策
- 1.1 Token 压缩拆三步地基,本批并行落前两步:A 暴露 provider 真实 usage、B 建 ContextStrategy 缝;压缩(C)留第三步 | 选:按 provider 层 / agent 层切(无依赖 + 改不同目录 = 可真并行)| 弃:压缩并入第二个(依赖 usage 不能并行)、单 change 串行 | 主导:用户(要「两 agent 同时 apply」)+ 主 agent 依赖分析 | 依据:code(ModelResponse 耦合面)
- 并行执行机制 = git worktree 隔离(mysteries-usage / -ctxstrat 各独立 target+HEAD),两 agent 各推一分支,主 agent merge 收口 | 主导:用户拍工作模式 + 主 agent | 依据:git-verify-shared-tree 教训
- A 解耦:ModelResponse 加 usage 用 Option<Usage> + 派生 Default(FinishReason::default()=Stop),不破全库裸构造点、压低与 B 在 agent/mod.rs 的交叠 | 弃:必填字段(破构造点)、tokenizer crate(违不扩 dep)| 主导:主 agent | 依据:code
- A usage 来源 = provider 真实回传:OpenAI include_usage + usage-only chunk(choices 空);Anthropic message_start(input)+message_delta(output)合成;缺失/非法降级 None | 主导:主 agent + 用户规划 D2 | 依据:design
- B trait 签名不带 usage(async + history→Vec<Message>):换 A/B 真并行(B 不引用 A 的 Usage),代价 = 压缩 change 要微调签名,§13「1.1 纯换实现」对签名不完全成立 | 选:并行优先 | 弃:带 last_usage 入参(依赖 A、YAGNI)| 主导:主 agent 取舍(并行 vs 缝终态)| 依据:design / YAGNI
- B 不 MODIFY agent-loop:Passthrough 等价 → 既有 requirement 仍真,新建 capability context-strategy,零回归测试锁定 | 主导:主 agent | 依据:design

## 变更
- A:src/provider/{mod,openai,stream,anthropic_stream,mock,wire}.rs + 既有 ModelResponse 构造点补 usage(agent/mod 测试、app、tui/mod、e2e);spec provider-abstraction/openai-transport/anthropic-transport 各 +1
- B:src/agent/context.rs(新:ContextStrategy/Passthrough/ContextError)+ agent/mod.rs(strategy 字段+set_strategy+两处 prepare 接线+From<ContextError>);新建 spec context-strategy
- 验证:两 worktree 各自 cargo test/clippy/fmt/validate 全绿;master 集成(stash 外部 TUI 后)cargo test 全绿;agent/mod.rs three-way 自动合并无冲突
- 编排:propose 主 agent 起草;两 agent 并行 apply;主 agent merge(checkout mock.rs 纯行尾噪音、完整保留外部 TUI 工作组)+ archive

## 待决
- add-token-compaction(1.1 第三步):接真实 usage(预计微调 ContextStrategy 签名加 last_usage)+ Compacting(分层保留 + summary + 正确性红线:不切断 tool_call↔result 配对)+ 把 From<ContextError> 细化为真正的 AgentError::Context variant(本批受「只动 agent」约束临时映射到 ProviderError::Transport)
- include_usage 对非 DeepSeek 兼容端点可能不回 usage → None,压缩须兜底(无用量退回不压/字符预估)
- 流程纪律:两 agent 本批均**跳过红灯停点①**(未停等确认即做完);主 agent 事后审(读码+自跑 cargo)通过,结果对但纪律未遵守,后续 dispatch 强化
- 主树外部 TUI 工作组(render +191/-30、8 snap、terminal、新 snap tui_tool_group_ctrl_o_hints)未提交,非本批,归属另一编排线
- 承前未动:git 身份 wanglei30 临时 + leafiellune purge;强制收尾前未 emit CallingModel 小瑕疵

## 引用
- changes:expose-token-usage / add-context-strategy(archive 2026-06-28-*)
- 前置:fix-transcript-viewport-clipping(17);后续:add-token-compaction(1.1 第三步)
- session:本会话——1.1 规划 + 拆分 + worktree 并行 apply 编排
