## Context

cut1→cut2b-a(均 archived)立起 TUI 四区 + 工具卡 + 全 phase + 全主题(`Theme` Midnight/Daylight + `buffer_to_styled` 带色快照)。本 change = **TUI 收官**:补 §8 完整观感的最后四样(C6 diff body / C7 致命框 / 滚动 / spinner)+ 一个 cut2b-a 待决配色 fix。用户已确认**一刀收官**(size 合 precedent、5 特性独立)。

现状(real code):`AppState` 持 `pending_permission: Option<PermissionRequest>`(其 `args` 派生 diff)、`tool_cards`(`ToolCardStatus::Running`→spinner)、`transcript`(含 `TranscriptBlock::Error`→C7)、`on_key`(仅权限键,无滚动);`render(frame, state, &Theme)`;`run_tui` 的 `select!`(crossterm `EventStream` ↔ `ui_rx`)。复用 `Theme` + `buffer_to_styled`。视觉权威:`theme.rs`/insta > `设计规范/01·02·03` > 原型 > 推断。

## Goals / Non-Goals

**Goals:**

- C6 diff body(args 派生、不读文件)、C7 致命框、transcript 滚动、spinner 动画、`[n·拒绝]` 配色 fix。
- diff/滚动/spinner 逻辑 red-green;渲染带色 insta;4 themed 帧对眼(完成 6 帧保真)。

**Non-Goals(留后续):**

- 内置命令 C8/C9、Anthropic、`ToolOutcome.exit` 恢复、256/16 降级(1.4)、运行时主题切换、step5 其余。

## Decisions

- **D1 C6 diff = `args` 派生纯函数,不读文件。** `compute_diff(tool_name, args) -> Vec<DiffLine{kind: Add|Del|Ctx, text}>`:`write_file` `content` 逐行 Add;`edit_file` `old_string` 逐行 Del + `new_string` 逐行 Add;`run_shell` 返回空(渲染端显命令)。**理由**:args 即 pending 动作真相,零文件 IO → 确定性、可 red-green(吸取 cut2a「别造数」);工作树上下文 diff 需读文件、非确定、1.0 不必要。只读工具自动放行不进框,故只 write/edit/shell 触 diff。

- **D2 C7 = 渲已有 `TranscriptBlock::Error`,无新事件。** `AgentEvent::Error` 已落 `TranscriptBlock::Error(msg)`(cut2a)。本 change 仅在 `render` 把它绘成致命框(`error.bg`/`border`/`fg` + title)。**理由**:§9 致命路径事件已在,渲染层补 C7 即可,不动 channel / agent-loop。

- **D3 滚动 = top-anchored offset,跟随 + 保持。** `scroll_offset`(可取首可见行 index 或距底行数,实现期定;**对外行为**:默认跟随底部、PageUp/Down 手动、非底时新内容保持位置、clamp [顶,底])。键位 **PageUp/PageDown**(避开输入框文本键;`02` 标滚动「1.0 自定」)。**只 transcript 区** offset 化(顶栏/状态行/输入框/权限框固定布局区)。offset 逻辑 red-green;`render` 据 offset 切 transcript 行窗口。

- **D4 spinner 确定性三分。** `AppState.spinner_frame: usize`;`advance_spinner()` = `frame=(frame+1)%FRAMES.len()`(纯,red-green);`render` 对 running 工具卡 / `CallingModel` / `ExecutingTool` 取 `FRAMES[spinner_frame]`(纯函数 of state → insta 设固定 index 确定);`run_tui` 的 `select!` 加 `tokio::time::interval(tick).tick()` 分支 → `app.advance_spinner()` + 重渲(**手动**)。**时间从不进 `render`/`AppState`**(仅 index 是状态)。FRAMES = braille `⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏`(ASCII `|/-\` fallback,`03`/`01` spinner=⚠️ adapt)。

- **D5 n-fix = 权限框动作行配色。** `[n · 拒绝]` → `error.fg`(`设计规范/01`「拒绝=error.fg」);`[y · 允许]` 维持。并入 C6 权限框渲染改动。

- **D6 测试分界。** `compute_diff` / `scroll_offset`(clamp/跟随/翻页)/ `advance_spinner` = **red-green**(`AppState`/纯函数,无终端);diff/C7/滚动窗口/spinner 固定帧 渲染 = **带色 insta**(`buffer_to_styled`);**4 themed 帧**(permission+diff / fatal × midnight/daylight)= **人工对眼**(`原型截图 02·03`);spinner tick / 终端 `select!` = **手动冒烟**。

## Risks / Trade-offs

- **[args 派生 diff 与真实文件不符]** edit 的 `old_string` 可能在文件里有多处 / 带上下文 → 缓解:1.0 只呈「将发生的替换」(args 声明的 old→new),不冒充工作树 diff;**诚实标注**为 args-diff,非 contextual diff。`edit_file` 工具本身已要求唯一匹配(§5.3),故 old_string 即将被替换的真实片段。
- **[spinner 动画测试不稳]** → 缓解:D4 时间不进 render/state,insta 锁固定帧、advance 走 red-green、tick 手动 —— 测试零时间依赖。
- **[滚动 offset 边界 off-by-one]** → 缓解:D3 clamp 规则明确 + red-green 覆盖 顶/底/跟随/保持 四路。
- **[4 帧迁移 / 新增的 insta diff]** → 缓解:权限态 / 致命态首现 diff/C7 → 新快照人工对眼一次,此后 diff 自动拦。

## Migration Plan

`AppState` 加 `scroll_offset` + `spinner_frame` + diff 计算;`render` 加 diff/C7/滚动窗口/spinner + n-fix 配色;`run_tui` 加 spinner tick + 滚动键。纯 tui 视觉 / 交互,不碰内核逻辑 / channel / agent-loop / config。回滚 = revert 本 change。无数据迁移。

## Open Questions

- 滚动键位是否加 Home/End(到顶/底)+ 行级 Up/Down(与未来输入历史键冲突?)—— 本 change 先 PageUp/PageDown,行级 / 输入历史留体验 change。
- spinner tick 频率(如 ~100ms)与「无 busy 态时是否暂停 tick」—— 实现期定;默认常驻 tick(advance 无害,render 静态态不显 spinner)。
- C7 title 文案(`AgentEvent::Error(String)` 是通用串)—— 渲为「致命错误」+ message;provider 特定 title(如 `03` C7 的 `ProviderError::Auth`)留事件携更多结构时细化。
