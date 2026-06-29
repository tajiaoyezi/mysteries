## 1. provider-registry 内置模型目录(TDD —— 新模块 / 新 API,红灯停点)

- [x] 1.1 【红】新建 `src/provider/registry.rs`(`provider/mod.rs` 挂 `pub mod registry;`)。写测试覆盖 spec:① `models_for("wps")` 含 `"zhipu/glm-5.2"` 等;② `models_for("anthropic")`=`["claude-opus-4-8"]`、`models_for("openai")`=`["gpt-5.5"]`、`models_for("deepseek")`=`["deepseek-v4-pro"]`;③ `models_for("my-llm")` → `None`;④ WPS/deepseek(kind 均 OpenAi)各返回自身集、不含 gpt-*。用最小桩(`models_for` 返回 `unimplemented!()` 或空)使编译、确认**红**(非编译错)→ 贴测试 + 红灯输出 **→ 停下等确认**
- [x] 1.2 【绿】实现 `CatalogEntry{ provider_id: &'static str, models: &'static [&'static str] }` + `const PROVIDER_CATALOG`(四家,wps 复用此前移除的 8 条)+ `models_for(id) -> Option<&'static [&'static str]>`(线性查 id,命中返回 `Some(models)`,未命中 `None`)
- [x] 1.3 【重构】清理

## 2. config 解析全部 provider profiles(TDD —— 新 API,红灯停点)

- [x] 2.1 【红】`config/mod.rs` 写 `resolve_provider_profiles` 测试覆盖 spec:① 新 schema 两 `[providers.*]` → 两条 profile(id/kind/base_url/model);② 旧单 provider 回落单条;③ 无 provider → 空;④ 某 profile 缺 kind/model → 跳过、其余正常。最小桩使编译、确认**红** → 贴测试 + 红灯 **→ 停下等确认**
- [x] 2.2 【绿】定义 `pub struct ProviderProfile{ id, kind, base_url, model, auth_type }`;`pub fn resolve_provider_profiles(raw: &RawConfig) -> Vec<ProviderProfile>`:`providers` 非空 → 遍历 map、`kind`&`model` 齐备者组 profile(缺则跳过);否则旧 `[provider]`+`model` 回落单条(缺则空)。**不**触碰既有 `resolve` / `resolve_multi_provider` / 运行时 `Config`
- [x] 2.3 【重构】清理(复用既有 `AuthType` 默认等)

## 3. ContextStrategy 钩子 + Compacting setter(TDD —— 新 trait 方法,红灯停点)

- [x] 3.1 【红】`compacting.rs` 写测试:① `Compacting::set_provider(new)` + `set_model("m2")` 后 `compact_now` → 摘要请求落 `new`、用 `"m2"`(MockProvider 记录请求断言);② `context.rs` 测试 `Passthrough` 经 `ContextStrategy::set_provider`/`set_model` 钩子后行为不变、不报错。最小桩(空方法体使编译)、确认**红** → 贴测试 + 红灯 **→ 停下等确认**
- [x] 3.2 【绿】`ContextStrategy` trait 加增量默认 no-op `fn set_provider(&mut self, _: Arc<dyn Provider>) {}` + `fn set_model(&mut self, _: String) {}`;`Compacting` override 之(设 `self.provider` / `self.model`)。`Passthrough` 用默认
- [x] 3.3 【重构】清理

## 4. Agent::set_provider + set_model 传播 strategy(TDD —— 新 agent 路径,红灯停点)

- [x] 4.1 【红】`agent/mod.rs` 写测试:① `set_provider(new)` 后一轮 `run` 模型请求落 `new`、旧不被调;② 装 `Compacting` 策略 → `set_provider(new)`/`set_model("m2")` 后该策略触发的自动压缩落 `new`/`"m2"`;③ `Passthrough` 下 `set_provider` 不报错、行为不变。最小桩、确认**红** → 贴测试 + 红灯 **→ 停下等确认**
- [x] 4.2 【绿】`Agent::set_provider(&mut self, Arc<dyn Provider>)`:设 `self.provider` + `self.strategy.set_provider(arc)`;扩 `set_model` 末尾加 `self.strategy.set_model(model.clone())`(闭合既有不传播 gap)
- [x] 4.3 【重构】清理;若有断言「set_model 不传播 strategy」的旧测试则更新(预期无)

## 5. UserInput::SetProvider + run_agent_task 热替 arm(TDD —— 新消息路径,headless Mock 驱动,红灯停点)

- [x] 5.1 【红】`tui/mod.rs` 写 `run_agent_task` 测试(Mock 驱动 · 无终端,仿既有 `run_agent_task_applies_set_model_*`):① 发 `SetProvider{id, model}`(id 在 profiles、凭据齐)后 `Prompt` → 该轮落新 provider/新 model、history 跨切换保留;② 未知 id → 收 `AgentEvent::Notice`、保持当前、task 不退;③ 缺凭据(`select_provider` Err)→ `Notice`、保持当前、不退。最小桩、确认**红** → 贴测试 + 红灯 **→ 停下等确认**
- [x] 5.2 【绿】`channel.rs` `UserInput` 加 `SetProvider{ id: String, model: String }`;`run_agent_task` 签名加 `profiles: BTreeMap<String, ProviderProfile>` + 凭据重建入口(`CliPaths`/工厂)+ 启动 `Config`(取 timeout/压缩旋钮);新增 arm:取 profile(缺→Notice)→ 组瞬时 `Config` → 重建 `CredentialChain` + `select_provider`(Err→Notice)→ `agent.set_provider`/`set_model` + 手动 `compacting` setter。`run_tui` 装配处补传 `profiles`(`resolve_provider_profiles`)
- [x] 5.3 【重构】清理;`select_provider` 若需按单 profile 重建,复用既有签名(瞬时 `Config`),不新增重复构造入口

## 6. 全量校验

- [x] 6.1 `cargo build` 通过、`cargo clippy --all-targets -D warnings` 零警告
- [x] 6.2 `cargo test` 全绿(新增 §1–§5 + 既有 agent/compacting/config/tui 测试不回归)
- [x] 6.3 `openspec validate add-provider-registry-hotswap --strict` 通过
- [x] 6.4 冒烟(隔离环境):构造含 `[providers.anthropic]` + `[providers.wps]` 的 config + 双凭据,在 agent-task 收 `SetProvider` 后确认下一轮落新 provider(以 diag 日志 / Mock 断言为准;无 TUI 入口,经测试驱动)
