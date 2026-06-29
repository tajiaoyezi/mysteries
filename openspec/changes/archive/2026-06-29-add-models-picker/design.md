## Context

epic ②(`add-provider-registry-hotswap`,已 pull)提供了 `/models` 的全部引擎:`models_for(id) -> Option<&[&str]>`(四家内置目录)、`provider_profiles_from_paths -> BTreeMap<id, ProviderProfile>`(已配 provider)、`UserInput::SetProvider{id, model}`(运行时热切,切主循环 + 自动/手动压缩三处,未知 id/缺凭据发 `Notice` 不崩)。本 change 是 epic 末步:补交互入口。TUI 已有两个浮层范式可复用——**命令补全浮层**(`CommandCompletion`,`↑↓/Enter/Esc`,`app.rs:415`)与 **C6 权限框**(框式、钉状态行上方)。`设计规范/` 无现成 picker 组件,本 change 新增 C12。

## Goals / Non-Goals

**Goals:**
- `/models` 打开模态 picker,分组列已配 provider × 模型,标记当前 active。
- `↑↓` 选 + 输入过滤 + `Enter` 选中发 `SetProvider` + `Esc` 取消。
- picker 状态机纯函数可单测;渲染 insta 快照。

**Non-Goals:**
- 不改引擎(`SetProvider`/`models_for`/profiles 读取均复用)。
- 不做模糊搜索(v1 substring)、不做输入历史(另线/路线图 1.4)、不动 `/model [name]`。

## Decisions

- **D1 分组布局 + 输入过滤**(用户拍板)。provider 名为**标题行(不可选)**,模型缩进列其下;`↑↓` 仅在**模型行**间移动(跳过标题、首尾环绕)。过滤:**不区分大小写 substring**,匹配 `"{id}/{model}"`;每次过滤后高亮重置到**首个可见模型行**;无匹配 → 显示空提示、`Enter` no-op。

- **D2 picker 状态机抽纯函数,与 ratatui 解耦。** 构建(profiles×catalog → 分组行)、过滤、`↑↓` 归约、`Enter`→选中 `(id, model)` 均为纯逻辑 → **纯函数单测**;渲染走 **insta 快照**(遵 CLAUDE.md「TUI 事后,不走 red-green」)。

- **D3 数据来源。** `provider_profiles_from_paths`(已配 provider)逐家:`models_for(id)` 为 `Some` → 列**目录全部**;为 `None`(custom)→ 列其 **profile 已配的那个 model**。标记当前 active 的 `(provider, model)` 行(● 当前)。

- **D4 键拦截优先级:picker > 命令补全 > 输入历史/滚动。** picker 打开时独占 `↑↓/Enter/Esc/字符/Backspace`(其余按键忽略或不影响);照 `handle_command_completion_key` 的拦截范式加 `handle_models_picker_key`,最先判断。关闭后键位恢复常态。

- **D5 选中只发 `SetProvider`,不自行改 session。** `Enter` → `UserInput::SetProvider{id, model}` → 关闭 picker;session 的 provider/model 由引擎 swap 后(经既有事件)更新,**单一入口**。**备选**:picker 直接 mutate session(弃:绕过引擎、与 ② 的 `SetProvider` 路径重复、易不一致)。

- **D6 浮层 = adapt C6 框式 + 新增 `设计规范/03` C12。** box-drawing 描边(设计规范许 adapt)、`accent` 高亮当前高亮行、`dim` 标题、`● 当前` 标记、footer `↑↓ 选 · Enter 切 · Esc 取消` + 过滤串回显。钉状态行上方(与 C6 一致)。

- **D7 `/models` 与 `/model [name]` 并存。** `/model <name>` = 快速直切当前 provider 的 model;`/models` = 浮层浏览**跨 provider** 切换。`/help`(C8)补列。

## Risks / Trade-offs

- **当前 active 的 provider id 来源** → 若 `SessionSnapshot` 只存 provider **display name** 而非 id,标记 ● 当前需从 profiles + 当前 model 推;实现期以 code 为准对齐(必要时让引擎/ session 暴露 active id)。
- **过滤清空当前高亮组** → 高亮重置首个可见模型行;全空 → 空提示、`Enter` no-op,不崩。
- **`↑↓` 既有占用**(补全 + 滚动)→ picker 打开时最高优先级独占,关闭恢复;本 change **不引入**输入历史(避免与未定的 1.4 抢 ↑↓ 语义)。
- **profiles 仅一家/为空** → 仍可开 picker(单家浏览其目录切 model);为空(理论上不会,TUI 必有 active)→ 空提示。
