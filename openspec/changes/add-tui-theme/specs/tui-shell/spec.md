## ADDED Requirements

### Requirement: 主题令牌 theme.rs(双调色板)

系统 SHALL 提供 `theme.rs`:`Theme` 结构持 `设计规范/01-设计令牌` 的全部语义 token(背景 / 描边 / 文字 / `accent.primary` / `success.fg` / `warning.fg` / `warning.bg` / `error.fg` / `error.bg` / `error.border` / `info.fg`),并提供 `Theme::midnight()` 与 `Theme::daylight()` 两套调色板,值为 `设计规范/01` 表的 `Color::Rgb`。token 值 MUST 由单测锁定(配色漂移 = 测试红)。

#### Scenario: token 值单测锁定

- **WHEN** 取 `Theme::midnight()` 与 `Theme::daylight()`
- **THEN** 各语义 token 的 `Color::Rgb` 等于 `设计规范/01` 表对应值(如 Midnight `accent.primary == Rgb(0xb1,0x8c,0xf0)`、Daylight `bg.base == Rgb(0xf4,0xf1,0xea)`),任一漂移使单测失败

### Requirement: themed 渲染

`render` SHALL 接受 `&Theme` 参数,各组件按语义 token 上色(替代 cut1/cut2a 的硬编码 ANSI 色):品牌 / 占位用 `text.muted`,prompt marker / tag / 工具名用 `accent.primary`,权限框用 `warning.fg`/`warning.bg`,工具卡 `✓` 用 `success.fg`、`✗` 用 `error.fg`,状态行 phase 按 `设计规范/02` 状态机配色。run_tui MUST 默认 `Theme::midnight()`。既有四区 / 工具卡 / phase 的**结构**不变。

#### Scenario: 同结构两主题异色

- **WHEN** 以 `Theme::midnight()` 与 `Theme::daylight()` 分别渲染同一 `AppState`
- **THEN** 两帧**文本结构一致**、**配色按各自调色板不同**(经带色快照可分辨)

### Requirement: 带色快照锁定(token 名)

系统 SHALL 提供带色快照表示 `buffer_to_styled(buffer, &Theme)`:在文本基础上,把每 cell 的 `fg`/`bg`(及关键 `Modifier`)**反查映射为语义 token 名**并注入快照,使 token **赋值错误**(用错 token)经快照 diff 暴露;token **值漂移**由 token 单测覆盖。既有 text-only 快照 MUST 迁移为带色表示(superset:文本 + 色注解)。

#### Scenario: 配色赋错被快照拦截

- **WHEN** 渲染产物里某区域的 token 赋值改变(如工具名从 `accent.primary` 误改为 `error.fg`)
- **THEN** 该区域的带色快照与锁定值不一致,测试失败(纯文本快照无法察觉此变化)

#### Scenario: welcome 两主题带色快照

- **WHEN** 以 Midnight 与 Daylight 渲染 welcome 态并 `buffer_to_styled`
- **THEN** 各得带色快照(文本 + token 名注解),与锁定一致;首帧经人工对 `原型截图/midnight-01-欢迎态` 与 `daylight-01-欢迎态` 审核后锁定
