## ADDED Requirements

### Requirement: 模型 picker 浮层

系统 SHALL 提供由 `/models` 触发的模态模型 picker 浮层。

**数据来源**:取自已配 provider profiles(`provider_profiles_from_paths`)× 内置目录(`models_for`):逐家 provider,`models_for(id)` 为 `Some` → 列**目录全部**模型;为 `None`(custom)→ 列其 profile **已配的那个 model**。SHALL 标记**当前 active** 的 `(provider, model)` 行。

**布局**:分组 —— provider 名为**不可选标题行**,模型缩进列其下。

**键位 / 交互**:`↑↓` 在**模型行**间移动(跳过标题行、首尾环绕);键入字符 / Backspace 实时**过滤**(不区分大小写 substring,匹配 `"{id}/{model}"`),过滤后高亮重置到**首个可见模型行**;`Enter` 选中高亮模型 → 发 `UserInput::SetProvider{ id, model }` 并关闭浮层;`Esc` 取消关闭(**不发消息**)。picker 打开时 SHALL **独占** `↑↓ / Enter / Esc / 字符 / Backspace`(优先于命令补全、transcript 滚动)。无匹配时 SHALL 显示空提示,`Enter` 为 no-op。

**测试边界**:构建(profiles×catalog→分组行)、过滤、`↑↓` 归约、`Enter`→选中 `(id, model)` 等 MUST 为**可单测纯逻辑**(不依赖真实终端);浮层渲染走 **insta 快照**。浮层样式 adapt `设计规范/` C6 框式(box-drawing 描边、钉状态行上方、footer `↑↓ 选 · Enter 切 · Esc 取消` + 过滤串回显);新增组件登记 `设计规范/03` C12。

#### Scenario: 构建分组列表并标记当前 active(纯函数)

- **WHEN** 以 profiles `{ wps: {model="zhipu/glm-5.2"}, openai: {model="gpt-5.5"} }`、当前 active = `(wps, "zhipu/glm-5.2")` 构建 picker 行
- **THEN** 得分组行:`wps` 标题 + 其目录 8 个模型(含 `zhipu/glm-5.2` 标 ● 当前)、`openai` 标题 + `gpt-5.5`;标题行不可选

#### Scenario: custom provider 列其已配 model(纯函数)

- **WHEN** profiles 含 `my-llm`(`models_for("my-llm") == None`,`model = "x-1"`)
- **THEN** `my-llm` 组仅列一行 `x-1`(custom 无目录,用已配 model)

#### Scenario: ↑↓ 在模型行间移动、跳标题、环绕(纯函数)

- **WHEN** 对 picker 行(含标题与模型混排)施加 `↑` / `↓`
- **THEN** 高亮只落在**模型行**(跳过标题);末模型再 `↓` 环绕到首模型,首模型再 `↑` 环绕到末模型

#### Scenario: 输入过滤缩小列表并重置高亮(纯函数)

- **WHEN** picker 打开后键入 `glm`
- **THEN** 仅匹配 `"{id}/{model}"` 含 `glm` 的模型行(及其 provider 标题)可见,高亮重置到首个可见模型行;键入无匹配串则显示空提示

#### Scenario: Enter 选中发 SetProvider(纯函数 / 注入)

- **WHEN** 高亮落在 `(wps, "zhipu/glm-5")` 时按 `Enter`
- **THEN** 产生 `UserInput::SetProvider{ id: "wps", model: "zhipu/glm-5" }` 且 picker 关闭;空匹配下 `Enter` 为 no-op

#### Scenario: Esc 取消不发消息(纯函数)

- **WHEN** picker 打开时按 `Esc`
- **THEN** picker 关闭,**不**产生 `SetProvider`(当前 provider/model 不变)

#### Scenario: picker 渲染快照(insta)

- **WHEN** 以一组 profiles + 某高亮 + 过滤串渲染 picker 浮层(`TestBackend`)
- **THEN** 快照含分组(标题 + 缩进模型)、当前 active 标记、高亮行、footer 键位提示;与基线快照一致
