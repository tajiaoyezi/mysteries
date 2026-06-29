## Why

`/models`(运行时浏览 / 热切「已 auth 的所有 provider」及其模型)需要两样现在**都不存在**的东西:① 一份「每家 provider 有哪些可选模型」的内置目录(picker 的数据源);② 运行时**换 provider** 的能力——当前 `Agent` 持 `provider: Arc<dyn Provider>` 在 `assemble_agent` 焊死,`set_model` 只改 model 串,无任何路径替换 provider 对象。本 change 是 `/models` epic 的**第 2 个**(地基 `add-multi-provider-config` 已 archive,持久化了多 provider;TUI 模态 picker 是第 3 个、依赖本 change)。

## What Changes

- **新增内置模型目录**:每家 provider → 其可用模型列表,作为运行时切换与后续 `/models` 浏览的数据源。目录键设计(per kind / per provider-id,WPS 复用 kind 但模型集不同)在 design.md 定。
- **resolve 暴露全部已配 provider profiles**:除现有「收敛单 active」外,新增「解析出所有 `[providers.<id>]` profiles」的能力,供浏览/切换(地基 change 的 design 已把这条明确留给本 change)。
- **`Agent` 运行时换 provider**:新增 `Agent::set_provider(Arc<dyn Provider>)`,对称于既有 `set_model`。
- **`Compacting` 运行时换 provider**:新增 `Compacting::set_provider(Arc<dyn Provider>)`,使切换后手动 `/compact` 走新 provider(切或不切由 design 定,默认同步切)。
- **新增 `UserInput::SetProvider` 消息 + `run_agent_task` 重建 arm**:收到后按选中 profile 重跑 `select_provider` 造新 `Arc<dyn Provider>`、热替进 agent(+ compacting)并同步 model。对称于既有 `UserInput::SetModel`。
- **不含 UI**:本 change 不加 `/models` 命令或 TUI 模态(那是 Change 3)。切换引擎仅由测试(Mock Provider)驱动。

## Capabilities

### New Capabilities

- `provider-registry`:内置 provider 模型目录——枚举每家 provider 的可用模型,作为运行时切换与 `/models` 浏览的数据源;目录为实现常量(随网关/官方模型变更改常量 + 单测,与既有 preset 默认 model 同策略)。

### Modified Capabilities

- `config-layering`:
  - **ADDED**「全部 provider profiles 解析」—— 在现有「收敛单 active」之外,暴露所有已配 `[providers.<id>]` profiles(id / kind / base_url / model)供浏览与切换。
- `agent-loop`:
  - **ADDED**「运行时 provider 切换」—— `Agent::set_provider(Arc<dyn Provider>)`,对称于既有「运行时模型切换」,下一轮请求用新 provider。
- `context-strategy`:
  - **ADDED**「Compacting 运行时 provider 切换」—— `Compacting::set_provider(Arc<dyn Provider>)`,使切换后压缩走新 provider。
- `tui-shell`:
  - **ADDED**「`UserInput::SetProvider` 变体与 agent-task 热替」—— 新消息变体携带切换目标;`run_agent_task` 新增 arm,按 profile 重跑 `select_provider` 热替 agent(+ compacting)并同步 model。对称于既有「`UserInput::SetModel` 变体」。

## Impact

- **代码**:
  - `src/provider/`(新模块,如 `registry.rs`):内置模型目录常量 + 查询 API。
  - `src/config/mod.rs`:`resolve` 旁路新增「解析全部 provider profiles」函数(运行时单 active `Config` 不变)。
  - `src/agent/mod.rs`:`Agent::set_provider`(provider 字段由焊死变可替换;现 `provider: Arc<dyn Provider>` 已是 `Arc`,改 `&mut self` setter)。
  - `src/agent/compacting.rs`:`Compacting::set_provider`。
  - `src/tui/channel.rs`:`UserInput` 加 `SetProvider` 变体。
  - `src/tui/mod.rs`:`run_agent_task` 新增 `SetProvider` arm(调 `select_provider` 重建)。
  - `src/app.rs`:可能需让 `select_provider` 可按单个 profile 重建(现按 `Config` 选,复用即可)。
- **不做(Change 3)**:`/models` 命令解析、TUI 模态 picker、↑↓ 导航、选中发 `SetProvider`。
- **测试**:headless 内核,强制 TDD —— 目录查询、`set_provider` 下一轮生效、`run_agent_task` 收 `SetProvider` 后热替(Mock Provider 脚本化 + 断言新 provider 被调用)、resolve 全 profiles。无网络无真实 FS。
- **凭据前提**:切到 provider X 需 X 的凭据(API key);无凭据时 `select_provider` 报错——错误如何呈现(notice / 拒绝)在 design 定。
