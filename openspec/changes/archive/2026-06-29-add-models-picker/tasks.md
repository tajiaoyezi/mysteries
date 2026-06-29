## 1. /models 命令(builtin-commands,TDD)

- [x] 1.1 【红】`tui/command.rs` 测试:`parse_command("/models") == Command::Models`;`/model claude` 仍 `Model(Some("claude"))`、`/model` 仍 `Model(None)`(二命令并存);命令元数据列表含 `/models`(供 `/help`)。运行确认失败
- [x] 1.2 【绿】`Command` +`Models` 变体;`COMMANDS` +`/models` 元数据(描述「浏览 / 切换 provider 与模型」);`parse_command` match
- [x] 1.3 【重构】清理

## 2. picker 状态机纯函数(TDD —— 新组件,红灯停点)

- [x] 2.1 【红】写 `ModelsPicker` 纯函数测试覆盖 spec(tui-shell):① `build_rows(profiles, active)` 分组行(provider 标题不可选 + 目录模型;custom 用已配 model;标 ● 当前);② `↑↓` 归约只落模型行、跳标题、首尾环绕;③ 过滤(不区分大小写 substring `"{id}/{model}"`)缩小 + 高亮重置首匹配 + 空匹配空提示;④ `Enter`→选中 `(id, model)` / 空匹配 no-op;⑤ `Esc` 取消不产生选中。先确认失败(非编译错;如需最小桩使编译)→ 贴测试 + 红灯输出 **→ 停下等确认**
- [x] 2.2 【绿】实现 `ModelsPicker` 纯逻辑:`build_rows`(`provider_profiles_from_paths` 结果 × `registry::models_for`)、`filter(input)`、`move_highlight(dir)`(跳标题环绕)、`selected() -> Option<(String, String)>`
- [x] 2.3 【重构】清理

## 3. 接线 + 渲染(TUI 外壳,事后快照,不走 red-green)

- [x] 3.1 `tui/app.rs`:`AppState` +`Option<ModelsPicker>`;`Command::Models` → 打开(取 profiles + 当前 active 构建);`handle_models_picker_key`(优先级 **picker > 命令补全 > 滚动**):`↑↓` 移动、`Enter` → `input_tx.send(UserInput::SetProvider{id, model})` + 关闭、`Esc` 关闭、字符 / Backspace → 过滤
- [x] 3.2 `tui/render.rs`:picker 浮层渲染(分组标题 dim + 缩进模型、`accent` 高亮、`● 当前` 标记、过滤串回显、footer `↑↓ 选 · Enter 切 · Esc 取消`;adapt C6 框式、钉状态行上方);加 `TestBackend` insta 快照(开 + 高亮 + 过滤几态)
- 注:§3 属 ratatui 渲染 / 交互接线,事后快照回归,不走 red-green;由 §2 纯函数单测 + §3.2 快照 + §5 手动冒烟兜底

## 4. 设计规范 + /help

- [x] 4.1 `设计规范/03-组件清单.md` +**C12 · models picker**(数据来源 / 状态 / 键位 / 终端渲染 / port·adapt·drop 三分类,对齐 C6 框式与 `01` 令牌)
- [x] 4.2 确认 `/help`(C8)渲染含 `/models`(§1.2 元数据落地的连带)

## 5. 全量校验

- [x] 5.1 `cargo build` 通过、`cargo clippy --all-targets -D warnings` 零警告
- [x] 5.2 `cargo test` 全绿(§1/§2 新测 + §3.2 快照 + 既有不回归)
- [x] 5.3 `cargo insta` 接受新快照;**首个 picker 快照须人工对照 `设计规范/`(C6 框式 + 令牌)审一次再 approve**
- [x] 5.4 手动冒烟:`cargo run` 进 TUI → `/models` → 出分组 picker、`↑↓` / 键入过滤 / `Enter` 切换(顶栏 / 状态行 model 变、引擎热切发 Notice)→ 再 `/models` → `Esc` 取消不变
- [x] 5.5 `openspec validate add-models-picker --strict` 通过
