## Why

两个小而独立的 TUI 操控缺口,用户决定合批一个 change 做(省一轮 propose/archive):

1. **输入框无历史**:发过的 prompt 无法召回,重输费事——类 shell 的 `↑↓` 翻历史是终端基本功。
2. **权限只有"逐次问"一档**:改动密集时每步都要按 `y` 很烦,且无法像 Claude 那样按场景放宽。需要 `normal / accept-edits / yolo` 三档,运行时可切。

## What Changes

- **输入历史 `↑↓`(本会话内存)**:主输入态(无浮层时)`↑` 翻上一条、`↓` 翻下一条已提交输入;翻到底恢复正在编辑的草稿;键入字符即脱离历史回到草稿。关闭 TUI 即清空(不落盘)。
- **权限三档模式**:
  - `normal` = 现状(改动类逐次弹 C6 权限框问 `y/n`)。
  - `accept-edits` = **文件编辑类(write/edit)自动放行**,`shell` 仍逐次问。
  - `yolo` = **全部自动放行、不弹框**(= `dangerously-skip-permissions`,命名沿用 codex 的 yolo)。
- **`Shift+Tab` 循环切模式**:crossterm 给成 `KeyCode::BackTab`(当前未占用),`normal → accept-edits → yolo → normal`;**状态行(C10)显当前模式**。
- **`PermissionLevel` 细分**:由 `{ReadOnly, RequiresConfirmation}` 改为 `{ReadOnly, Edit, Execute}`,让工具自身声明改动类别(accept-edits 据此区分"放行编辑 / 仍问 shell")。**内部 trait 契约变更**,触及全部 `Tool` 实现(非用户可见 breaking)。
- **自动放行时跳过 C6 权限框**:`accept-edits`(对 edit)/`yolo` 下,`gate` 直接放行,不再弹框、不阻塞。

**不做**(明确排除,见 design):`plan-mode`(耦合 agent-loop,单独 change)、模式持久化(每次启动回 `normal` 安全档)、输入历史落盘。

## Capabilities

### New Capabilities

(无——均修改既有 capability)

### Modified Capabilities

- `tool-system`:`PermissionLevel` 由 `{ReadOnly, RequiresConfirmation}` 细分为 `{ReadOnly, Edit, Execute}`;工具据此声明是文件改动还是命令执行。
- `permission-gate`:新增 `PermissionMode {Normal, AcceptEdits, Yolo}` 与 auto-allow 策略(mode × level → 自动放行 / 询问);decider 运行时读取当前模式决定是否弹框。
- `tui-shell`:输入历史 `↑↓` 召回(本会话内存);`Shift+Tab` 循环切权限模式;状态行显当前模式;模式为自动放行时不弹 C6 权限框。

## Impact

- **代码**:
  - `src/tool/{mod,edit,shell,fs}.rs`:`PermissionLevel` 细分 + 各工具声明 `Edit`/`Execute`/`ReadOnly`。
  - `src/permission/mod.rs`:`PermissionMode` + 纯函数 `auto_allows(mode, level) -> bool`(**headless 强制 TDD**);`gate` 仍按 level 派发,自动放行逻辑落 decider。
  - `src/tui/channel.rs`:`ChannelDecider` 持**共享模式句柄**(`Arc<...>`),`decide()` 先查 `auto_allows` 命中即 `Allow` 不走 channel。
  - `src/tui/app.rs`:输入历史 reducer(纯函数)+ `↑↓` 接入主输入态 + `BackTab` 循环切模式 + 共享模式句柄读写。
  - `src/tui/render.rs`:状态行(C10)追加 `MODE:<mode>` 段。
  - `设计规范/03-组件清单.md`:C10 补模式指示、C11 补 `↑↓` 历史(引 `02-布局与交互.md` 键位);C6 注明自动放行时不渲染。
- **依赖**:零新依赖。
- **测试**:`PermissionMode × PermissionLevel` 策略 + `PermissionLevel` 细分 = headless 强制 TDD(红灯停点见 tasks);输入历史 reducer = 纯函数单测;状态行 `MODE` 渲染 = insta 快照(TUI 事后)。
- **设计规范偏差(port/adapt/drop)**:状态行加 `MODE` 段 = **adapt C10**(原型无此字段);输入历史 `↑↓` = 纯行为新增,无新视觉(**port**,引 `02` 键位);自动放行跳过权限框 = C6 语义补充。
