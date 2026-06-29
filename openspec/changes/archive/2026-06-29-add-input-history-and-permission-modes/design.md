## Context

两个独立小特性合批(用户决定),横跨三个 capability:`tui-shell`(输入历史 + 模式切换键/指示)、`permission-gate`(模式策略)、`tool-system`(PermissionLevel 细分)。现状:

- **权限门**(`src/permission/mod.rs`):二值。`gate()` 对 `ReadOnly` 直接放行,`RequiresConfirmation` 交 `PermissionDecider`。无任何模式。
- **TUI decider**(`src/tui/channel.rs:121` `ChannelDecider`):`decide()` 发 `PermissionRequest` 经 channel → TUI 显 `pending_permission` → 用户 `y/n` → oneshot 回送。
- **agent loop**(`src/agent/mod.rs:186`):每个 tool_call 调 `gate(&call, tool, decider)`。
- **输入**(`src/tui/app.rs:855+`):`self.input` 单行 `String`,Enter 提交;`↑↓` 仅被命令补全 / models picker 浮层 handler 占用(它们 early-return),**主输入态 `↑↓` 空闲**。`ctrl+o` 在 `on_key` 顶部处理,pending 权限框展示时仍生效(已有测试 `ctrl_o_toggles_during_pending_permission`)。
- **工具**:`edit.rs` 两工具 + `shell.rs` 一工具 = `RequiresConfirmation`;`fs.rs` 四读类 = `ReadOnly`。
- **键位可用性**:`Shift+Tab` 经 crossterm 在 Windows console 给成 `KeyCode::BackTab`,当前未占用。

## Goals / Non-Goals

**Goals:**
- 主输入态 `↑↓` 翻已提交输入(本会话内存),草稿可恢复。
- `normal / accept-edits / yolo` 三档,`Shift+Tab` 循环切,状态行显当前模式,运行时即时生效。
- 模式策略为纯函数(headless 强制 TDD);输入历史导航为纯函数(单测)。

**Non-Goals:**
- `plan-mode`(耦合 agent-loop / system prompt,单独 change)。
- 模式持久化(每次启动回 `normal` 安全档)、输入历史落盘。
- 多行输入(另线;但本 change 的 `↑↓` 语义需为其留位,见 R4)。

## Decisions

- **D1 三档模式 = 纯函数 `auto_allows(mode, level) -> bool`(headless)。** 矩阵:`Normal`→非 ReadOnly 全询问;`AcceptEdits`→`Edit` 放行、`Execute` 询问;`Yolo`→`Edit`/`Execute` 全放行。落 `src/permission/mod.rs`,**强制 TDD**(mode×level 全覆盖)。`ReadOnly` 不进此函数(门提前放行)。

- **D2 `PermissionLevel` 细分 `{ReadOnly, Edit, Execute}`。** 选:工具自身声明类别(类型安全,编译期强制每个工具选 variant)。**弃**:decider 按工具名 allow-list 判 edit/shell(脆弱、新工具漏配静默失效);**弃**:保留 `RequiresConfirmation` 再加平行 `category()` 方法(多一根轴、与 level 冗余)。代价:触全部 `Tool` 实现 + `gate()` match + MockTool/既有测试——但纯机械,编译器兜底。

- **D3 模式态归属 = `ChannelDecider` 持共享句柄,`gate()` 签名不变。** 模式是 **TUI 运行时态**(`Shift+Tab` 实时切),decider 在 TUI 层是天然 owner;`gate()` 在 headless agent loop 无 UI 访问,**不**给它加 mode 参数。`ChannelDecider` 构造时注入 `Arc<Mutex<PermissionMode>>`(或 `Arc<AtomicU8>`),`decide()` 读快照 → `auto_allows` 命中即 `Allow` **不走 channel**(yolo 下零阻塞、零弹框);否则照旧 oneshot 往返。`AppState` 持**同一** `Arc`(`Shift+Tab` 写 + 渲染读),与 decider 共享。**弃**:`gate(mode, …)`(把运行时 UI 态下推进 headless 门、签名污染、还要把 Arc 穿进 agent loop)。

- **D4 `Shift+Tab`(`BackTab`)在 `on_key` 顶部处理,先于浮层。** 与 `ctrl+o` 同档:任意 phase(含 pending 权限框)可切。循环 `Normal→AcceptEdits→Yolo→Normal`,只写共享 `Arc`,纯逻辑(切换序列可单测)。

- **D5 输入历史 = 纯 reducer + 本会话内存。** `AppState` += `input_history: Vec<String>` + `history_cursor`(`None`=草稿态)+ `draft` 暂存。提交时 push(连续去重)、游标归 `None`。主输入态(到达 `on_key` 末段 `match key.code`,浮层 handler 已 early-return)`↑`→上一条、`↓`→下一条/越过最新恢复 `draft`;字符/Backspace 输入 → 游标归 `None`(脱离历史)。导航逻辑抽纯函数单测(类比 models picker 状态机)。

- **D6 独立底部模式行(新组件,非状态行段)。** 模式**不**塞进状态行 C10,而是 `layout_rows` 末尾新增一行 `Constraint::Length(1)`(状态行下方、屏幕最末),由 `render_mode_line` 渲染 `<glyph> <mode> · shift+tab 切换`:glyph 按模式 `▸`/`▸▸`/`▲`(均 width-1,避 emoji),配色 `text.muted`/`accent.primary`/`warning.fg`;常驻显示(含 `Normal`)令 `shift+tab` 提示可发现。状态行 `status_meta_spans` 去掉 `MODE` span 并修掉其遗留的前导 ` · `。`设计规范/03` 新增模式行组件条目(C13)、C10 删模式段、C11 补 `↑↓` 历史、C6 注「自动放行时不渲染」;`设计规范/02` 补键位 + 底部行格式。事后 insta 快照。
  - **弃**:塞进状态行 C10(用户实测嫌挤、要求单起一行类 Claude Code);**弃**:照搬 Claude 绿橙配色(改用本项目 theme token)。

## Risks / Trade-offs

- **`BackTab` 终端可用性**:Windows console 经 crossterm 可靠给 `KeyCode::BackTab`;SSH/部分 Unix 终端可能不传。本 change **不提供** `/mode` 命令兜底(用户只要 Shift+Tab);若日后跨平台需要,再补命令入口。**已记为待决。**
- **共享模式态并发**:decider 在 agent-task(async)读、TUI 线程写 → `Arc<Mutex>`/`Atomic` 保证;`decide()` 读一次快照即可,无需长持锁。
- **`PermissionLevel` 细分的回归面**:改 enum 触发 `gate()` match、MockTool、既有 permission/tool 测试与可能的快照更新——必须全绿;编译器会逼出每处遗漏。
- **`accept-edits` 正确性依赖工具如实声明 `Edit`/`Execute`**:新增工具须选对 variant;类型系统强制选择(不会忘),但选**错**仍会错放行——审查时核对各工具 level。
- **`↑↓` 与未来多行输入冲突**:多行下 `↑↓` 要移光标。本 change 约定 `↑↓`=历史(单行);多行落地时改为「光标在首行才翻历史,否则移光标」——届时改 R(输入历史)的触发条件,不推翻 reducer。**已知,记入路线图。**
- **合批的代价**:一条 change 扛三个 spec delta + 两套 TDD 规格(权限走严格红绿、历史走纯函数单测)。archive 时横跨 `tool-system`/`permission-gate`/`tui-shell` 三个 spec——用户知情决定。
