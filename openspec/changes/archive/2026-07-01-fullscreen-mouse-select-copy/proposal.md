## Why

用户要在 TUI 同时拥有:**全屏布局 + 鼠标滚轮滚动 + 拖选复制(无需 shift)+ `↑↓` 输入历史**。终端鼠标协议下「滚轮」与「原生拖选」互斥——收滚轮必须捕获鼠标(无"只收滚轮"模式),捕获后拖拽被程序收走,终端做不了原生拖选。**唯一同时拥有的办法是程序自管选区与复制**(opencode 即如此:捕获鼠标 + 自绘选区 + 自写剪贴板)。

本 change 在**全屏基线**(`HEAD`,alt-screen + 已捕获鼠标)上新增「app 鼠标拖选 + 复制」,并**废弃 `inline-render-core`**(内联渲染因布局观感不达预期被否,回全屏)。剪贴板用 **arboard**(本机 Windows 直写系统剪贴板,绕开 OSC52 在 WT 上的已知不稳;不走 SSH 故 arboard 比 OSC52 更可靠)。

## What Changes

- **废弃内联改造**:丢弃 `inline-render-core` 的未提交代码(`git stash` 留底),回 `HEAD` 全屏基线(alt-screen + `EnableMouseCapture` 已在 `terminal.rs`)。同步移除 `inline-render-core` 的 change 目录(随 stash 留底)。
- **app 选区状态**:`AppState` 加 `selection: Option<Selection>`(锚点 + 当前点,屏幕 cell 坐标)与拖拽活动标志;纯逻辑可单测(起点 / 扩选 / 规范化 start≤end / 清除)。
- **鼠标事件**:事件循环处理 `MouseEventKind::Down(Left)`(起选)/ `Drag(Left)`(扩选)/ `Up(Left)`(**松开即复制**);现有滚轮 `ScrollUp/Down` 保留。
- **复制**:`Up(Left)`(松开即复制)与 **`Ctrl+C`(有选区时)**把选区文本写入系统剪贴板(arboard),**复制后保留选区高亮**;纯空白 / 空选区跳过复制不覆盖剪贴板。优先级 **模态 > 选区 > 退出**:`pending_permission` 时 `Ctrl+C` 维持原、无选区时维持原退出(`should_exit`)——避免误删退出路径、避免选区凌驾模态。
- **选区取文**:从**渲染 buffer**(`CompletedFrame.buffer`)读选区 cell 的 symbol,**按显示宽度跳延续格**(延续格 symbol 是空格 `" "`,不能靠空串判定),按行 join(start 行自 start 列、中间行整行、end 行至 end 列),cell 访问用 `Option`/clamp 防越界 panic。v1 仅限**可见视口**内选区;选已滚出内容(拖动自动滚)留后续。
- **选区高亮**:`render` 给选区内 cell 叠加 selection 背景色(`Theme` 加一个 `selection.bg` token)。
- **选区清除**:新拖选 / 任意滚动(滚轮 + 键盘 PageUp/Home/End 等)/ `Resize` / 提交输入 / `/clear` / 单击未拖动 / `Esc`(模态优先)时清除;**复制本身不清除**(松开后高亮保留,与终端原生一致)。

## Capabilities

### New Capabilities

(无独立新 capability)

### Modified Capabilities

- `tui-shell`:
  - **ADDED**:`鼠标拖选与复制`(捕获鼠标下 app 自管选区:Down/Drag/Up 归约 + 高亮渲染 + 松开/Ctrl+C 复制且保留高亮 + 从 buffer 按显示宽度取文本 + arboard 写剪贴板;纯逻辑选区归约可单测、高亮可 `TestBackend` 快照、取文可单测)。
  - **MODIFIED**:`运行中可中断(Esc 中断本轮)` 的分流 —— `Ctrl+C` / `Esc` 优先级 **`pending_permission`(模态)> 选区 > 中断/退出**:有选区时 `Ctrl+C` 复制并保留选区、`Esc` 清选区(均不退出),无选区维持原语义;pending 时不因选区改变模态。
  - **MODIFIED**:`鼠标滚轮滚动(捕获鼠标)` —— `Down/Drag/Up(Left)` 归 app 选区(不再「其余 kind 一律忽略」)、滚轮滚动时清选区、终端原生框选让位于 app 自管拖选(无需 Shift)。

## Impact

- **依赖**:**新增 `arboard`**(跨平台系统剪贴板,本机直写;理由:OSC52 在用户 WT 上不稳、本机不走 SSH)。**用户已批准**;加依赖后以 `cargo tree` 复核传递依赖(Windows 下 `clipboard-win` / `windows-sys` 为主),异常膨胀再回报。
- **代码**:
  - `src/tui/terminal.rs`:保持 alt-screen + `EnableMouseCapture`(无改动,确认即可)。
  - `src/tui/app.rs`:`Selection` 状态 + 归约 + `Ctrl+C` 复制接线;清除时机。
  - `src/tui/mod.rs`:事件循环 `Down/Drag/Up(Left)` 处理 + `Ctrl+C` 复制分流(`should_exit` 前拦截)。
  - `src/tui/render.rs`:选区高亮叠加 + `buffer → 选区文本` 提取(跳宽字符延续格)。
  - `src/tui/theme.rs`:加 `selection.bg` token(双调色板)。
- **废弃**:`inline-render-core`(未提交,随 stash 留底);其 change 目录移除。inline 的所有改动(commit.rs 提交时机、内联 terminal、render 拆分等)作废。
- **测试**:选区归约 / 取文纯逻辑单测;选区高亮 `insta` 快照;arboard 写剪贴板在单测中以 trait/注入隔离(不真写系统剪贴板)。
- **风险**:① 屏幕 cell↔文本映射在宽字符 / 换行处易错(用渲染 buffer 直读规避反推);② arboard 在某些环境(headless CI)初始化可能失败 —— 复制失败须**静默降级**(发 Notice / 不 panic),不阻塞主循环;③ 选已滚出内容需拖动自动滚,v1 不做(仅可见区)。
