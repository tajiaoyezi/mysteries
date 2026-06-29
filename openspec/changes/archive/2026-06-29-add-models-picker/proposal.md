## Why

`/models` epic 的引擎已就位:② `add-provider-registry-hotswap` 落地了 provider 注册表(`models_for`)、已配 profiles 读取(`provider_profiles_from_paths`)、运行时热切(`UserInput::SetProvider{id, model}`)。但缺**交互入口**——用户无法浏览/选择。本 change(epic 第 ③ 步)加 `/models` TUI 模态 picker:列出已配 provider 及其模型,`↑↓` + 输入过滤选中 → 发 `SetProvider` 热切。

## What Changes

- **新命令 `/models`**(与 `/model <name>` 并存)→ 打开模态 picker。
- **picker 数据**:`provider_profiles_from_paths`(已配 provider)× `models_for(id)`(内置目录;custom 无目录命中则列其**已配的那个 model**),标记**当前 active** 的 (provider, model)。
- **分组布局**:provider 名作标题行(不可选)+ 模型缩进列其下;`↑↓` 在**模型行**间移动(跳过标题、首尾环绕)。
- **输入过滤**:键入字符实时缩小列表(不区分大小写 substring,匹配 `id/model`);Backspace 退格;高亮重置到首个匹配。
- **键位**:`Enter` 选中高亮 → 发 `UserInput::SetProvider{id, model}` → 关闭 picker(引擎热切、发 Notice);`Esc` 取消关闭;字符/Backspace 改过滤。
- **浮层样式**:adapt `设计规范/` **C6 权限框**的框式浮层(box-drawing 描边、钉状态行上方、`↑↓/Enter/Esc` 提示),复用**命令补全浮层**的列表交互;新增 `设计规范/03` 组件条目 **C12 · models picker**;`/help`(C8)补列 `/models`。

## Capabilities

### New Capabilities

(无)

### Modified Capabilities

- `builtin-commands`:**ADDED** `/models` 命令(打开模型 picker,区别于 `/model [name]`)。
- `tui-shell`:**ADDED** 模型 picker 浮层(数据来源、分组布局、输入过滤、`↑↓/Enter/Esc` 键位、选中发 `SetProvider`)。

## Impact

- **代码**:
  - `src/tui/command.rs`:`/models` → `Command::Models`;`/help` 元数据补 `/models`。
  - `src/tui/app.rs`:`AppState` +`Option<ModelsPicker>`;键拦截优先级 **picker > 命令补全 > 输入历史/滚动**;构建(profiles×catalog)/ 过滤 / `↑↓` 移动(跳标题环绕)/ 选中→`SetProvider` 逻辑(**纯函数,可单测**)。
  - `src/tui/render.rs`:picker 浮层渲染(分组、当前态标记、过滤行、高亮、footer)。
  - `设计规范/03-组件清单.md`:+C12。
- **依赖**:复用 epic ② 引擎(已 pull:`models_for` / `provider_profiles_from_paths` / `UserInput::SetProvider`),**零新依赖**。
- **测试**:picker 状态机走**纯函数单测**(构建 / 过滤 / 移动 / 选中);渲染走 **insta 快照**(TUI 事后,不走 red-green)。
- **不做**:模糊搜索(v1 用 substring)、跨 provider 模型去重、目录按协议端点分组(沿用 ② 假设)。
