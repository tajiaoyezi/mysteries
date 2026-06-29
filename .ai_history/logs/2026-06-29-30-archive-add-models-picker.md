# 2026-06-29 · 30 · archive add-models-picker

## 决策

- **`/models` epic ③:TUI 模态 picker(交互入口)** | 主导:用户(「/model 增强 = 列出所有已配 provider + 当前 provider 模型,↑↓ 切换」)| 依据:code(epic ② 已落 `models_for` / `provider_profiles_from_paths` / `UserInput::SetProvider`,只缺入口)+ spec(② design 把「补交互入口」留给本 change)
- **D1 分组布局 + 输入过滤**(用户拍板,二选一里选「分组 + 加过滤」)| provider 名为**标题行(不可选)**,模型缩进列其下;`↑↓` 仅在**模型行**间移动(跳标题、首尾环绕);过滤 = **不区分大小写 substring** 匹配 `"{id}/{model}"`,每次过滤高亮重置到首个可见模型行,无匹配 → 空提示 + `Enter` no-op | 弃:平铺列表(provider 归属不清)、模糊搜索(v1 用 substring)
- **D2 picker 状态机抽纯函数,与 ratatui 解耦** | 构建(profiles×catalog→分组行)/ 过滤 / `↑↓` 归约 / `Enter`→`(id,model)` 均纯逻辑 → **纯函数单测**;渲染走 **insta 快照**(遵 CLAUDE.md「TUI 事后,不走 red-green」)| 依据:CLAUDE.md TDD 边界
- **D4 键拦截优先级:picker > 命令补全 > 输入历史/滚动** | picker 打开独占 `↑↓/Enter/Esc/字符/Backspace`,照 `handle_command_completion_key` 范式加 `handle_models_picker_key` 最先判断;关闭恢复常态。**本 change 不引入输入历史**,避免与未定路线图 1.4 抢 `↑↓` 语义
- **D5 选中只发 `SetProvider`,不自行 mutate session** | `Enter` → `UserInput::SetProvider{id,model}` → 关 picker;session 的 provider/model 由引擎 swap 后经既有事件更新,**单一入口** | 弃:picker 直接 mutate session(绕过引擎、与 ② 的 `SetProvider` 路径重复、易不一致)
- **审查中发现的 wrap 算术假绿(checkpoint A)**:`move_highlight` 测试用 `for _ in 0..20` + 断言回到首行,对 9 个模型行不是 9 的整数倍 → 误过 → 改为**边界环绕直测**(走到末行→move(1)→断言首行;首行→move(-1)→断言末行)
- **两处实测 bug 修复(主 agent 独立 review 出,折进本 change feat 提交)**:① picker 渲染早期用全宽 `clear_area` → 框右侧露**终端默认黑带**(实测难看)→ 改 `frame.render_widget(Clear, area)` 只清 56 宽框区;② 状态栏经 `provider.name()` 显 wire name `openai` 而非逻辑 id → `SessionSnapshot.provider = config.provider.id`,picker `● 当前` 与状态栏对齐显 `wps`
- **审查(独立 cargo/clippy + 读码,非信完成声明)**:验四点全过 —— 框只 Clear 不溢出黑带、状态行逻辑 id、快照框内 `bg.surface` 干净无欢迎屏 bleed、`Esc` 不发 `SetProvider`(`rx.try_recv().is_err()`);`cargo test` 320 lib + 1 e2e passed、clippy 零警告;无返工

## 变更

- `src/tui/command.rs`:`/models` → `Command::Models`;`/help` 元数据补 `/models`
- `src/tui/app.rs`:`AppState` +`Option<ModelsPicker>`;`ModelsPicker`(`build_rows`/`filter`/`move_highlight`/`selected`/`visible_rows`/`highlighted_row`/`push_filter_char`/`shows_empty_hint`)+ `ModelsPickerRowKind::{ProviderHeader,Model}` + `handle_models_picker_key` + `resolve_active_provider`
- `src/tui/render.rs`:`render_models_picker`(分组、`● 当前` 标记、过滤行回显、高亮、footer `↑↓ 选 · Enter 切 · Esc 取消`);只 `Clear` 框区
- `src/tui/mod.rs`:`SessionSnapshot.provider = config.provider.id`(逻辑 id)
- `设计规范/03-组件清单.md`:+C12 · models picker
- spec:`builtin-commands` ADDED `/models` 命令;`tui-shell` ADDED 模型 picker 浮层 requirement
- 验证:`cargo test` 320 lib + 1 e2e passed / 2 ignored;`cargo clippy --all-targets -D warnings` 零警告;`openspec validate --strict` 过

## 待决

- 输入历史 `↑↓`(路线图 1.2 子项)—— 独立小 change,主输入态 `↑↓` 现空闲,接入干净
- 模糊搜索(v1 用 substring;需要再升级)
- 目录是否按协议端点分组(沿用 ② 假设)

## 引用

- change:`add-models-picker`(D1–D7 见 design.md;archive 路径 `changes/archive/2026-06-29-add-models-picker`)
- 前置 change:`add-provider-registry-hotswap`(29,epic ② 引擎:`models_for`/`provider_profiles_from_paths`/`SetProvider`)、`add-multi-provider-config`(27,多 provider 持久化地基)
- session 主导:用户「/model 增强 = 列已配 provider + ↑↓ 切换」→ propose(D1 布局问用户拍板)→ 子 agent implement(红灯停点;checkpoint A 修 wrap 假绿)→ 主 agent review(独立 cargo/clippy + 读码)→ 实测两 bug(黑带 + wire name)修复折进 feat → 主 agent 复核通过
