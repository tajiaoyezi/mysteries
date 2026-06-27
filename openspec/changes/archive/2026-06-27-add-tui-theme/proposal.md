## Why

cut1/cut2a 把 TUI 四区 + 工具卡 + 全 phase 渲出来了,但用的是**硬编码 ANSI 色**(`Color::Cyan`/`Yellow`/`Red`/`DarkGray`),且快照(`buffer_to_text`)**只锁文本、不含色** —— 配色漂移测不出,离 `设计规范/01-设计令牌` 的 Midnight/Daylight 语义色板还差一层。本 change(**TUI cut2b 的 a 半**)上**全主题** + **带色快照基建**:`theme.rs` 双调色板 + token単測,`render` 吃 `&Theme` 语义上色,`buffer_to_styled` 让快照捕获 cell 样式。余下组件(C6 diff / C7 致命框 / 滚动 / spinner)留 cut2b-b(`add-tui-components`)。

## What Changes

- **`src/tui/theme.rs`(新)**:`Theme` 结构持 `设计规范/01` 全部语义 token(`bg.base` / `accent.primary` / `success.fg` / `warning.fg` / `error.fg` / `text.muted` …),`Theme::midnight()` / `Theme::daylight()` 返 `Color::Rgb`(01 表的 hex)。
- **token単測**:断言两套调色板每个 token 的 Rgb 值(漂移 = 测试红,`设计规范/01` 落点)。
- **`render(frame, state, &Theme)`**:签名加 `&Theme`,把硬编码色全替为语义 token(C1 品牌 dim、C3 `>` marker = `accent.primary`、tag = `accent.primary`、权限框 = `warning.fg`/`warning.bg`、工具卡 glyph `✓`=`success.fg`/`✗`=`error.fg`、状态行 phase 色 …);更新 3 处调用点(`render.rs` 测试 + `mod.rs` run_tui 两处 draw,run_tui 默认 `Theme::midnight()`)。
- **带色快照基建**:新 `buffer_to_styled(buffer, &Theme)` —— 在 `buffer_to_text` 的文本基础上,把每 cell 的 `fg`/`bg`(+ `Modifier`)**反查映射为语义 token 名**(如 `‹accent.primary›工具名‹/›`)。`buffer_to_text` 退役。
- **迁移既有 6 快照 → styled**:welcome / permission / phase_status_lines / tool_card running·done·error 的 `.snap` 重生成为带色注解版(一次性 `cargo insta review` 重核)。
- **Daylight 渲染 + welcome themed 快照 ×2**:测试传入两套 Theme 渲 welcome,锁 Midnight + Daylight 两帧;welcome themed 人工对眼。

### 4 点定夺(已与你确认 Option 1)

1. **拆分**:本 = cut2b-a(主题 + 带色基建);cut2b-b = C6 diff(args 派生)+ C7 + 滚动 + spinner。**6 帧对眼分布**:welcome×2 在本 change;permission(+diff)×2 + fatal(C7)×2 在 cut2b-b(那两帧的组件 b 才有)。
2. **带色 insta** → `buffer_to_styled` 反查 **token 名**。**两层锁色**:token **值**漂移 → **token単測**直接抓;token **赋错**(该 accent 却用 error)→ **styled insta** 抓。既有 6 text 快照**迁移为 styled**(superset:文本 + 色注解),一次性重核。
3. **主题选择** → `render(frame, state, &Theme)`,默认 Midnight(run_tui 内置);两套主题经测试传入即可对眼,**无需 config 字段**;运行时切换(config `theme` / `/theme` 命令)**延后** → **不动 `config-layering`**。**capability:只 MODIFIED `tui-shell`**。
4. **C6 diff 来源**(定夺,落地在 cut2b-b)→ **args 派生、不读文件**:`write_file` `content`→`+`;`edit_file` `old_string`→`−` + `new_string`→`+`;`run_shell` 显命令无 diff。零文件 IO、确定性、可 red-green(吸取 cut2a「别造数」,args 即 pending 动作真相)。

**port/adapt/drop(cut2b-a · 主题)**:port ✅ = `设计规范/01` 语义色板(token 值)、glyph;adapt ⚠️ = truecolor `Rgb`(256/16 色降级**延后路线图 1.4**,本 change 默认 truecolor)、font-weight→`Modifier::BOLD`、dim→`DIM`、选中/hover→`REVERSED`;drop ❌ = (本 change 纯配色,无新增阴影/动画;spinner 留 cut2b-b)。

**明确不含**(留后续):C6 diff body / C7 致命框 / transcript 滚动 / spinner(cut2b-b);256/16 色降级(1.4);运行时主题切换;内置命令、Anthropic、`ToolOutcome.exit` 字段恢复(属 tool-system,收尾)、step5 其余。

## Capabilities

### New Capabilities

<!-- 无。本 change 扩展既有 tui-shell,不新建 capability。 -->

### Modified Capabilities

- `tui-shell`: ADDED —— 主题令牌(`theme.rs` Midnight/Daylight + token単測)、themed 渲染(`render` 吃 `&Theme` 语义上色)、带色快照锁定(`buffer_to_styled` 捕获 token 名,既有快照迁移)。cut1/cut2a 既有 requirement(四区 / 工具卡 / phase 的**结构**)不变,本 change 在其上**叠加配色锁**。

## Impact

- **改动代码**:新 `src/tui/theme.rs`;`src/tui/render.rs`(签名 + 语义上色 + `buffer_to_styled`,退役 `buffer_to_text`)、`src/tui/mod.rs`(run_tui 默认 Midnight 传 `&Theme`);迁移 `src/tui/snapshots/*.snap`(6 → styled)+ 新增 Daylight welcome 快照。
- **新增依赖**:**无**(纯逻辑 + 既有 ratatui/insta)。
- **构建 / 测试**:token 值走**単測**(两套调色板纯值);themed/styled 渲染走**带色 insta**;welcome themed **人工对眼**(`原型截图/midnight-01` + `daylight-01`)。`cargo test` 默认全绿、无终端。既有 render 测试随签名更新、随迁移重核。
- **里程碑**:本 change 后 TUI 呈 `设计规范/01` 语义配色 + 配色漂移由测试拦截;cut2b-b 补 diff/C7/滚动/spinner 即 §8 完整观感。
- **下游契约**:`Theme` + `buffer_to_styled` 供 cut2b-b 的新组件直接 themed + 带色锁。
