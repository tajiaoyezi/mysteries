## Context

`/models` epic 的第 2 个 change(地基 `add-multi-provider-config` 已 archive)。当前运行时**无法换 provider**:

- `Agent` 持 `provider: Arc<dyn Provider>`(`agent/mod.rs:72`),在 `assemble_agent`(`app.rs:133`)焊死;`set_model`(`agent/mod.rs:98`)只改 `model` 串。
- `select_provider`(`app.rs:52`)由单 active `Config` 构造 `Arc<dyn Provider>`,凭据名 = `config.provider.id`。
- `resolve`(`config/mod.rs:257`)只收敛**单 active** provider;多 provider 已持久化(`[providers.<id>]`)但运行时只暴露一家。
- **provider 被 clone 进三处**:① `agent.provider`(主循环);② `agent.strategy`(一个 `Compacting`,自动压缩上下文钩子);③ `run_agent_task` 的 `compacting: Option<Compacting>` 参数(手动 `/compact`)。三者各持独立 `Arc` clone + `model` 串。

约束:headless 内核,强制 TDD(Mock Provider 驱动);权威次序 code/编译器/测试 > spec;不含任何 TUI(Change 3)。

## Goals / Non-Goals

**Goals:**
- 内置模型目录:provider-id → 可用模型列表,作为运行时切换与 `/models` 浏览的数据源。
- `resolve` 旁路暴露**全部**已配 provider profiles(id / kind / base_url / model),运行时单 active `Config` 不变。
- 运行时换 provider:`Agent::set_provider` + `Compacting::set_provider`/`set_model` + `UserInput::SetProvider{id, model}` + `run_agent_task` 重建 arm(经 `select_provider` 热替),切换连贯(三处 provider 全部跟随)。

**Non-Goals:**
- **不**做 `/models` 命令 / TUI 模态 / ↑↓ 导航 / 选中发消息(Change 3)。
- **不**改 `auth login` 交互流程、`write_config`、运行时 `Config` 结构。
- **不**实装 WPS OAuth2(独立线)。

## Decisions

- **D1 内置模型目录键 = provider-id(用户拍板,弃 ProviderKind / base_url)。** 新增 `src/provider/registry.rs`:`const PROVIDER_CATALOG: &[CatalogEntry]`,`CatalogEntry{ provider_id: &'static str, models: &'static [&'static str] }`;`models_for(id) -> Option<&'static [&'static str]>`。键与 auth preset / config `[providers.<id>]` 的 id 对齐:`anthropic` / `openai` / `deepseek` / `wps`。**custom**(用户自取 id)无目录命中 → `None`,picker(Change 3)对其只列已配 model。
  - 目录内容(实现常量,随网关/官方变更改常量 + 单测,与既有 preset 默认 model 同策略):`anthropic → [claude-opus-4-8]`;`openai → [gpt-5.5]`;`deepseek → [deepseek-v4-pro]`;`wps →` 复用此前从 `cli.rs` 移除的 8 条(`zhipu/glm-5.2`、`zhipu/glm-5`、`moonshot/kimi-k2.5`、`deepseek/deepseek-v4-pro`、`deepseek/deepseek-v4-flash`、`ali/qwen3.7-max`、`xiaomi/mimo-v2.5-pro`、`google/gemini-3.5-flash`)。
  - **弃 ProviderKind**:WPS / deepseek 均 `kind=OpenAi` 但模型集各异,按 kind 会错拿 gpt-*。**弃 base_url**:自定义端点 base_url 不在目录,且 preset id 已天然区分。

- **D2 `resolve` 旁路暴露全部 profiles。** `config/mod.rs` 新增 `pub struct ProviderProfile{ id, kind, base_url, model, auth_type }` + `pub fn resolve_provider_profiles(raw: &RawConfig) -> Vec<ProviderProfile>`。新 schema → 遍历 `providers` map;旧单 provider → 回落为单条;无 → 空。**跳过不完整 profile**(缺 `kind`/`model` 不可切,filter 掉而非整体报错——一家坏掉不拖垮整列)。运行时 `Config` / `resolve` / `select_provider` 零改动。

- **D3 `SetProvider` 载荷 = `{id, model}`,`run_agent_task` 重解析(用户拍板,弃自包含 profile)。** `UserInput::SetProvider{ id: String, model: String }`。`run_agent_task` 新增持有:(a)`profiles: BTreeMap<String, ProviderProfile>`(启动时 `resolve_provider_profiles` 得),(b)凭据工厂(每次切换重建 `CredentialChain`,拾取新增 key),(c)启动 `Config` 的运行时旋钮(`timeout_secs`/压缩设置)。收到 `SetProvider{id, model}`:
  1. `profiles.get(&id)` 缺 → `AgentEvent::Notice("未知 provider …")`,继续(不崩)。
  2. 组瞬时 `Config{ provider: ProviderConfig{id, kind, base_url, auth_type}, model, ..启动旋钮 }`。
  3. 重建 `CredentialChain` → `select_provider(&transient, creds)`;`Err`(如缺 key)→ `Notice(错误)`,继续。
  4. `agent.set_provider(arc.clone())`;`agent.set_model(model.clone())`;`if let Some(c)=&mut compacting { c.set_provider(arc); c.set_model(model) }`。
  - **弃自包含**:provider 构造逻辑应留 headless(`select_provider` 单一入口);picker(Change 3)只发 id+model。

- **D4 切换连贯 = 三处 provider 全部跟随。** `Agent::set_provider(arc)` 除设 `self.provider`,**也同步内部 strategy**;`Agent::set_model` 同样同步 strategy 的 model(顺带闭合既有 `set_model` **不**传播到 strategy 的潜在 gap)。手段:`ContextStrategy` 加**增量默认 no-op** 钩子 `fn set_provider(&mut self, _: Arc<dyn Provider>) {}` + `fn set_model(&mut self, _: String) {}`;`Compacting` override 之,`Passthrough` 用默认(无 provider,no-op)。`run_agent_task` 的手动 `compacting` 因是具体 `Compacting`,直接调其 setter。
  - **理由**:用户显式切走某家后,中途自动压缩或手动 `/compact` 不得再打旧 provider(其 key 可能正是切走的原因)。**弃只换主循环**:留 strategy / 手动 compacting 在旧 provider 不连贯。**弃 rebuild 整个 strategy**:需把 `CompactionSettings` 多线进 `run_agent_task`;mutate setter 线程更少。

- **D5 无凭据 / 未知 id 切换 = 发 `Notice` 不崩、保持当前 provider。** 切换失败是「功能态」非 panic;沿用既有 `AgentEvent::Notice` 通道。Change 3 的 picker 另可预过滤为「已有凭据」者,但引擎层必须自身兜底。

## Risks / Trade-offs

- **`ContextStrategy` 加 provider/model 钩子轻微泄漏**(Passthrough 无 provider 概念)→ 默认 no-op,仅 Compacting 反应;文档注明「只有携带 provider 的策略响应」。
- **目录内容硬编码、易过期** → 实现常量 + 单测锁定,随网关/官方变更同既有 preset 默认 model 策略更新;`models_for` 未知 id 返回 `None` 而非空 slice(区分「无目录」与「目录为空」)。
- **瞬时 `Config` 须正确继承运行时旋钮**(timeout / 压缩)→ 测试断言切换后 timeout/压缩设置不丢(从启动 Config 带入)。
- **每次切换重建 `CredentialChain` 读盘** → 频次极低(人手切换),且利于拾取新 auth 的 key;可接受。
- **`set_model` 行为扩展(现在传播 strategy)触及既有受测代码** → 视为 bug 闭合,补测试断言传播;若有断言「不传播」的旧测试需更新(预期无)。

## Open Questions

- 目录是否需区分「按协议端点」(WPS 同一 id 两协议端点模型集是否一致)——沿用 `add-wps-ai-provider` 既有假设「8 模型两端点都可用」,如后续分化再改目录为按协议分组;本 change 不预埋。
