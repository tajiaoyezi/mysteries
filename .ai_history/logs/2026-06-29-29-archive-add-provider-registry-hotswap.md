# 2026-06-29 · 29 · archive add-provider-registry-hotswap

## 决策

- **`/models` epic ②:provider 注册表 + 运行时换 provider 热切** | 主导:用户(「继续开发」→ 选 epic ②③ 顺序做,本为②)| 依据:code(`Agent` 持 `provider: Arc<dyn Provider>` 在 `assemble_agent` 焊死、`set_model` 只改 model 串、无 `set_provider`)+ spec(地基 `add-multi-provider-config` design 已把「resolve 全 profiles」留给本 change)
- **D1 内置模型目录键 = provider-id** | 选:per provider-id(`anthropic`/`openai`/`deepseek`/`wps`;custom 自取 id 无目录命中 → picker 只列已配 model)| 弃:per `ProviderKind`(WPS/deepseek 同 `kind=OpenAi`,按 kind 会错拿 gpt-*)、per base_url(custom 端点不在目录)| 主导:用户拍板。`models_for(未知)` 返回 `None` 非空 slice。目录内容复用此前从 `cli.rs` 移除的 8 条 WPS 模型 + 各 preset 默认 model(实现常量)
- **D3 `SetProvider{id, model}` + task 重解析** | 选:消息只带 id+model;`run_agent_task` 持 `profiles` map + 每次重建 `CredentialChain`,经**单一** `select_provider` 入口由瞬时 `Config`(继承 startup 的 timeout/压缩旋钮)造 provider | 弃:消息自包含 `{kind,base_url,model}`(provider 构造逻辑下推 UI 层、多入口)| 主导:用户拍板
- **D4 切换三处 provider 全跟随** | 主循环 `agent.provider` + 自动压缩 strategy + 手动 `compacting`;手段:`ContextStrategy` 加**增量默认 no-op** 钩子 `set_provider`/`set_model`,`Compacting` override,`Passthrough` 用默认;`Agent::set_provider`/`set_model` 传播 strategy —— 顺带**闭合既有 `set_model` 不传播 strategy 的潜在 gap** | 主导:讨论收敛(主 agent 提、用户批 D4)| 依据:code(provider 被 clone 进三处)| 弃:只换主循环(留 strategy/手动 compacting 在旧 provider,切走的家仍可能被压缩调用)、rebuild 整 strategy(需把 `CompactionSettings` 多线进 task)
- **D5 未知 id / 缺凭据切换 = 发 `AgentEvent::Notice` 不崩、保当前 provider** | 早返 `Err`→`Notice`,mutate 前全部短路;缺凭据用 `credentials.resolve(id).is_none()` **快速预检**(因 `select_provider` 仅在请求时惰性失败)| 依据:code
- **审查(主 agent 独立复现 + 逐行读码,非信完成声明)**:查出并要求修 4 项 —— ① `cli.rs` 越界 `cargo fmt` churn(revert);② **缺凭据测试 env 耦合**(用 id `openai` → 查真实 `OPENAI_API_KEY`,设了该 env 的机器假失败 → 改 id `wps`(`_ => None` 恒空),**must-fix**);③ tasks.md §4.1② 指定的「strategy 传播」测试被换成 Passthrough no-op → 补 `RecordingSwitchStrategy` 真测背书 agent-loop spec scenario;④ manual compacting 集成测带理由跳过(setter 已 `compacting.rs` 单测)。**无 correctness bug**;`apply_set_provider` 同步执行无竞态窗口

## 变更

- 新建 `src/provider/registry.rs`:`PROVIDER_CATALOG`(四家)+ `models_for`;`provider/mod.rs` 挂 `pub mod registry`
- `config/mod.rs`:+`ProviderProfile` + `resolve_provider_profiles`(旁路,跳不完整 profile;**不动** `resolve`/`resolve_multi_provider`/运行时 `Config`)
- `agent/context.rs`:`ContextStrategy` +默认 no-op `set_provider`/`set_model` 钩子;`agent/compacting.rs`:`Compacting` setter + trait override;`agent/mod.rs`:`Agent::set_provider` + `set_model` 传播 strategy
- `tui/channel.rs`:`UserInput::SetProvider{id, model}`;`tui/mod.rs`:`RunAgentTaskConfig` + `apply_set_provider` 热替 arm;`app.rs`:`load_merged_raw` + `provider_profiles_from_paths`
- spec:ADDED `provider-registry`(新);`config-layering`/`agent-loop`/`context-strategy`/`tui-shell` 各 ADDED 一 requirement
- 验证:`cargo test` 309 lib + 1 e2e passed / 2 ignored;`cargo clippy --all-targets -D warnings` 零警告;`openspec validate --strict` 过

## 待决

- **Change ③(epic ③)**:`/models` TUI 模态 picker(↑↓ 浏览 provider+模型、选中发 `SetProvider`)—— 紧随本 change
- 目录是否需按协议端点分组(沿用 `add-wps-ai-provider` 假设「WPS 8 模型两协议端点都可用」;分化再改目录结构)
- 目录内容硬编码,随网关 / 官方模型变更改常量 + 单测

## 引用

- change:`add-provider-registry-hotswap`(D1–D5 见 design.md;archive 路径 `changes/archive/2026-06-29-add-provider-registry-hotswap`)
- 前置 change:`add-multi-provider-config`(27,多 provider 持久化地基 + 「resolve 全 profiles 留给本 change」)、`add-wps-ai-provider`(26,WPS provider + `WPS_MODELS` 来源)
- session 主导:用户「继续开发」→ 拆 `/models` epic ②③ → 本为②;propose(D1/D3 两决策问用户拍板)→ 子 agent implement(红灯停点)→ 主 agent review(独立 cargo/clippy + 读码,4 项修复)→ 修复 agent → 主 agent 复核通过(独立复现 + 验 4 项 + 无新越界/假绿)
