## Why

cut1→cut2b-a 把 TUI 四区 + 工具卡 + 全 phase + 全主题(Midnight/Daylight 令牌 + 带色快照)都立起来了,但还差 §8 完整观感的最后几样:权限框还没 **diff body**、致命错误还没 **C7 框**、transcript **不能滚动**、busy 态用静态字符没 **spinner**。本 change(**TUI 收官**)一并补齐这四样 + 一个 cut2b-a 抽查发现的配色待决项,做到 §8 完整观感。复用 cut2b-a 的 `Theme` + `buffer_to_styled`,零新依赖。这之后 TUI 即完成,1.0 仅剩内置命令 / Anthropic / 收尾。

## What Changes

- **C6 diff body**(`设计规范/03` C6):权限框体渲染 **args 派生**的 diff —— `write_file`:`args.content` 整段作 `+`(add);`edit_file`:`args.old_string` 作 `−`(del) + `args.new_string` 作 `+`(add);`run_shell`:显示命令、无 diff。**不读文件**(确定性,吸取 cut2a「别造数」,args 即 pending 动作真相)。行色:add=`success.fg` / del=`error.fg` / ctx=`text.body`。
- **C7 致命错误框**(`设计规范/03` C7,§9):把已有 `TranscriptBlock::Error`(由 `AgentEvent::Error` 落入)渲为致命框 —— `error.bg` 底、`error.border` 描边、`error.fg` 文,title 标致命(§9 致命路径,Loop 已终止)。
- **transcript 滚动**:`AppState` 加 `scroll_offset` —— 默认跟随底部(新内容自动到底)、手动 **PageUp/PageDown** 滚(滚到非底部时新内容不强拉回底)、offset clamp 在 [顶, 底];**只 transcript 滚**(顶栏 / 状态行 / 输入框 / 权限框固定)。键位见 `设计规范/02`(原型未覆盖滚动,1.0 自定后纳入快照)。
- **spinner 动画**:running 工具卡 + `CallingModel` / `ExecutingTool` phase 用动画 spinner(braille `⠋⠙⠹…`,ASCII `|/-\` fallback)替 cut2a 的静态字符。`AppState.spinner_frame` 帧 index;`render` 吃当前帧(确定性);`run_tui` 经 `tokio::time::interval` tick 推进。
- **n-fix**(cut2b-a 待决项):权限框动作行 `[n · 拒绝]` 配色 → `error.fg`(`设计规范/01`「拒绝 = error.fg」)。

### 4 点定夺(已与你确认:一刀收官)

1. **拆分** → 一刀(`add-tui-polish`);size 合 precedent、5 特性互相独立。
2. **spinner tick 确定性** → **时间从不进 `render`/`AppState`**:`render` 吃 `AppState.spinner_frame`(纯函数,insta 测设**固定 index** → 确定快照);`advance_spinner`(index `+1 mod N`)= **red-green**;`run_tui` 的 `select!` 加 `interval.tick()` 分支驱动 advance + 重渲 = **手动**。显 spinner 的态:running 工具卡 + `CallingModel` / `ExecutingTool`;Idle/done/error/WaitingForPermission = 静态 glyph。
3. **滚动模型** → 跟随底部 + 手动 PageUp/Down;offset clamp [顶, 底];滚到非底时新内容保持位置(不强拉);只 transcript 滚。offset 逻辑 **red-green**,切片渲染 **insta**。
4. **对眼覆盖** → diff/C7 完成 **4 themed 帧**:permission(+diff) `midnight/daylight-02` + fatal(C7)`midnight/daylight-03`(+ cut2b-a 的 welcome×2 = **6 帧设计保真齐**);spinner 固定帧 = 结构 insta(无原型帧、不对眼)。

**port/adapt/drop(cut2b-b)**:port ✅ = C6 diff 结构(±/ctx + 行色,`03` C6)、C7 框结构(`03` C7)、滚动跟随语义(`02`);adapt ⚠️ = spinner braille→ASCII fallback、圆角→box、按钮→`[n·拒绝]` 文本;drop ❌ = 阴影 / 渐变 / 鼠标滚轮(键位滚动替代)。

**明确不含**(留后续):内置命令(C8/C9)、Anthropic、`ToolOutcome.exit` 字段恢复(工具卡 exit foot,属 tool-system 变更)、256/16 色降级(1.4)、运行时主题切换、step5 其余收尾。

## Capabilities

### New Capabilities

<!-- 无。本 change 扩展既有 tui-shell,不新建 capability,不碰 config-layering / agent-loop。 -->

### Modified Capabilities

- `tui-shell`: ADDED —— C6 diff body(args 派生)、C7 致命错误框、transcript 滚动、spinner 动画;并修正权限框 `[n·拒绝]` 配色。cut1→cut2b-a 既有 requirement(四区 / 工具卡 / phase / 主题 / 带色锁)不变,本 change 叠加。

## Impact

- **改动代码**:`src/tui/{app, render, mod}.rs`(diff 计算 + scroll_offset + spinner_frame 于 `AppState`;diff/C7/滚动/spinner 渲染于 `render`;`run_tui` 加 spinner tick + 滚动键);新增 / 迁移 `snapshots/*`。
- **新增依赖**:**无**(`tokio` `time` 已在;复用 cut2b-a `Theme` + `buffer_to_styled`)。
- **构建 / 测试**:C6 diff 计算 / 滚动 offset / spinner 帧推进 = **red-green**(`AppState` 逻辑);diff/C7/滚动/spinner 渲染 = **带色 insta**;**4 themed 帧人工对眼**(`原型截图 02·03` × midnight/daylight)。`cargo test` 默认全绿、无终端;tick / 终端循环 = 手动冒烟。
- **里程碑**:本 change 后 TUI 达 §8 完整观感(6 帧设计保真齐);**TUI 工作收官**。
