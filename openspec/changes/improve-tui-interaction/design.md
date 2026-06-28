## Context

现状(已对代码核实,`src/tui/`):

- **滚动底座已就绪**:`app.rs` 有 `scroll_offset` / `follows_bottom` / `visible_scroll_offset`(底部自动跟随)+ 原语 `scroll_up` / `scroll_down`(行级步进 N 行)/ `page_up` / `page_down`(整页);`mod.rs` 事件循环用 `tokio::select!` 分发 `Event::Key` / `Event::Mouse`,`handle_scroll_key` 目前**只** match `PageUp`/`PageDown`,`handle_scroll_mouse` 把 `ScrollUp`/`ScrollDown` 映射为行级步进(`MOUSE_SCROLL_LINES = 3`)。
- **鼠标捕获已正确开启**:`terminal.rs` 进入时 `EnableMouseCapture`、退出/panic 时 `DisableMouseCapture`,且在 alternate screen + raw mode 下;`mod.rs` 已处理 `Event::Mouse(ScrollUp/Down)`。**即:crossterm「收滚轮」的标准配方在我方已齐全。**
- **工具卡**:`TranscriptBlock::Tool(ToolCard)`(单一时间线,上个 change 已并入)在 `render.rs::tool_card_lines` **全量**渲染头(glyph+名+args+只读徽章)/ 体(output 行,截断标记)/ 脚(`exit {code}`)。无折叠态。
- **键位**:`↑`/`↓`/`Home`/`End` 当前**无人处理**——`handle_scroll_key` 不 match 它们,`on_key` 的 `match key.code` 只接 `Char`/`Backspace`/`Enter`,其余落 `_ => {}`。输入框是单行、无输入历史,不消费方向键。

约束(CLAUDE.md / config.yaml):纯 Rust;视觉以 `设计规范/` 为准、行为以 code/test 为准;headless 内核强制 TDD,TUI 渲染走事后 insta;`ctrl+o` / 折叠态 / 新键路径属「新键路径 / 新状态字段」,implement 阶段须先写失败测试、贴红灯、停下等审(见 tasks.md 红灯停点)。

行业参照:claude-code 工具输出默认折叠、`ctrl+o`(或 `ctrl+r`)全局展开;滚动靠键盘 + 滚轮双通道。

## Goals / Non-Goals

**Goals:**
- 工具卡默认折叠为单行,`ctrl+o` 全局 toggle 展开/折叠所有工具卡;只折叠 `Tool` 块。
- 键盘滚动全覆盖:`↑`/`↓` 行级、`PageUp`/`PageDown` 整页、`Home` 到顶、`End` 回底并恢复底部跟随。
- 给出 Windows Terminal 滚轮无响应的**根因结论**与**诚实降级路径**;不引入未经真机验证的「修复」。

**Non-Goals(本 change 明确不做):**
- 工具卡**单条**展开/折叠(需 transcript 焦点/选中模型,本期无;留后续,见决策 ①.d)。
- `Assistant` / `User` / 命令块的折叠(仅折叠 `Tool` 块)。
- markdown 渲染、diff 语法高亮(技术方案 §13 1.4 的其余加法项,另开 change)。
- 为绕过 ConPTY 限制而自写 Win32 `ReadConsoleInputW` 鼠标输入路径(决策 ③ 评估后否决)。
- 输入历史(`↑`/`↓` 调历史命令):本期把方向键判给滚动;若将来加输入历史再分语境(决策 ②)。

## Decisions

### 决策 ① 工具卡折叠模型(默认态 / 折叠形态 / 全局 vs 单条 / 折叠范围)

**已定(本 change 锁定):**

- **a. 默认折叠**:`AppState` 增 `tools_expanded: bool`,默认 `false`(折叠)。`render` 据此对**所有** `Tool` 块二选一渲染:折叠=单行头;展开=现有全量(头/体/脚,含截断标记)。
- **b. 全局 toggle**:`ctrl+o`(`KeyCode::Char('o')` + `CONTROL`,仅 `KeyEventKind::Press`)翻转 `tools_expanded`。**仅全局**,不针对单条。
- **c. 折叠范围**:**只折叠 `Tool` 块**。`Assistant`/`User`/`Error`/`Help`/`Status`/`Notice` 不受影响。理由:上个 change 刚修好「最终回答(`Assistant` 末块)钉底可见」,折叠正文会重新害可见性。
- **d. `ctrl+o` 拦截位置**:必须在 `on_key` 的 `KeyCode::Char(ch) => self.input.push(ch)` arm **之前**拦截,否则 `ctrl+o` 会把 `'o'` 打进输入框。

**折叠单行形态(推荐默认,列为待拍板 → 见 Open Questions A1):**

- 折叠行 = `设计规范/03` C5 的**头行**:`{glyph} {name} {args 摘要}` + 右侧**结果摘要**。
  - `glyph`:running→spinner / done→`✓` / error→`✗`(沿用现有)。
  - 结果摘要(**推荐**):done 且有 output → 追加 ` · {N} 行 ⌄`(N=output 行数,`⌄` 提示可展开);有 `exit` → 追加 ` · exit {code}`(非 0 用 `error.fg`);running → ` · 运行中…`。`⌄` 是「有隐藏内容、`ctrl+o` 可展开」的可发现性 affordance。
- **备选**:① 折叠仍保留前 1~2 行 output 预览 + `⋯ +N 行`(claude-code 部分风格)——折叠后高度不定、与「单行」语义模糊,**弃**;② 纯单行、无任何结果提示——最干净但用户不知有无隐藏内容、`ctrl+o` 不可发现,**弃**。推荐取「单行头 + 行数/exit + `⌄` 提示」折中。

**为何只全局、不单条**:单条展开需要「当前选中哪张卡」的 transcript 焦点/光标模型,而现有 transcript 无 selection 概念(`scroll_offset` 只是行窗口)。引入焦点态成本高且改动面大,本期按 claude-code 的全局 toggle 落地,单条留后续(Open Questions A2)。

### 决策 ② 键盘滚动键位归属(滚动 vs 输入编辑,冲突分析)

**键位表(本 change 锁定):**

| 键(仅 Press) | 归属 | 行为 | 原语 |
| --- | --- | --- | --- |
| `↑` / `Up` | transcript 滚动 | 上滚 1 行 | `scroll_up(.., 1)` |
| `↓` / `Down` | transcript 滚动 | 下滚 1 行;触底恢复 `follows_bottom` | `scroll_down(.., 1)` |
| `PageUp` / `PageDown` | transcript 滚动 | 整页(不变) | `page_up` / `page_down` |
| `Home` | transcript 滚动 | 到顶(`scroll_offset=0`,`follows_bottom=false`) | `scroll_to_top`(**新**) |
| `End` | transcript 滚动 | 回底 + 恢复底部跟随(`follows_bottom=true`) | `scroll_to_bottom`(**新**) |

**冲突分析(为何无歧义):**

- 事件循环里 `handle_scroll_key` 在 `on_key` **之前**执行;方向键/Home/End 被 `handle_scroll_key` 先吃掉、`return true`,**不会**流到 `on_key` 的文本编辑路径。输入框只认 `Char`/`Backspace`/`Enter`,本就不消费这些键 → **无冲突**。
- `设计规范/02`「交互/键位」明示「滚动/翻页/输入历史(Up/Down/PageUp)等键位**原型未覆盖;1.0 实现自定,纳入快照后锁定**」——本决策即在该授权缝内自定。
- **取舍 / 待拍板(→ Open Questions B1)**:把 `↑`/`↓` 判给「transcript 滚动」而非「输入历史」。理由:本期无输入历史功能,方向键空置;滚动是当下刚需。**风险**:将来加输入历史时,`↑`/`↓` 的语义要回收/分语境(如「输入框非空或有历史指针时 `↑` 调历史,否则滚动」),届时需再开 change 调整键位 + 迁移快照。先记此账。

新增原语 `scroll_to_top` / `scroll_to_bottom` 为纯逻辑(操作 `scroll_offset` / `follows_bottom`),走**强制 TDD**(headless 内核)。

### 决策 ③ Windows Terminal 滚轮无响应:根因结论 + 方案取舍(诚实可达程度)

**根因结论(调研后定论):这是 ConPTY / Windows 构建相关的平台限制,不是我方配置错误,也不是 crossterm 的能力缺陷。**

证据链(权威来源):

1. **我方配方已正确**:crossterm 收滚轮需 `enable_raw_mode` + `EnableMouseCapture`(+ 进 alternate screen),我方 `terminal.rs` 全齐;`mod.rs` 也已处理 `Event::Mouse(ScrollUp/Down)`。键盘事件能收到 = `EventStream` 通路本身没问题。故**排除「配置缺失」**。(参 crossterm docs:Mouse events 默认关闭,须 `EnableMouseCapture`;turborepo PR #11487 的教训正是「不开 capture 则终端吞滚轮作自身 scrollback」——我方没犯这个错。)
2. **平台层是真瓶颈**:Windows Terminal 经 **ConPTY** 托管子程序。`microsoft/terminal` #376(ConPTY mouse input)原话:**“ConPTY won't transit mouse reports … from a hosted application”**;因为 ConPTY 还要兼容期待 `MOUSE_EVENT` 走 `ReadConsoleInput` 的原生 Win32 console app,需要做 VT⇄`INPUT_RECORD` 翻译,这块长期缺位。`microsoft/terminal` #545 进一步列出「Terminal 合成鼠标序列 / ConPTY 读鼠标序列」两处都要补。
3. **该平台缺陷已在较新 Windows 修复(版本相关)**:`PowerShell/Win32-OpenSSH` #1863 实测——conhost `10.0.22000.348` 复现「子程序收不到鼠标」,升级到 `10.0.22523.1000` 后**修复**;微软官方 tips(MS Learn,2025-11 更新)也述「Windows Terminal 支持 VT input 应用的鼠标输入(tmux / mc 可识别),鼠标模式下按 `Shift` 才做框选」。**即:够新的 Win11(conhost ≳ 10.0.22523)上,我方现有滚轮管线本应能收到事件;偏旧的 Win10 / 早期 Win11 上,ConPTY 根本不转发,滚轮被 Terminal 吞作 scrollback,crossterm 永远等不到 —— 与我方代码无关。**

**结论:纯 crossterm 无法「根治」所有 Windows Terminal 版本的滚轮**——能否收到取决于 ConPTY/Windows 构建。诚实定位:**滚轮 = 尽力而为(环境相关);键盘滚动 = 与鼠标无关、保证全覆盖的兜底。**

**方案选项与取舍:**

| 选项 | 做法 | 取舍 | 取舍结论 |
| --- | --- | --- | --- |
| **C1 接受平台限制 + 键盘兜底** | 不动鼠标管线;把 B 键盘滚动确立为全覆盖兜底;文档/提示说明滚轮环境相关 | 零风险、诚实、可移植;不「修复」旧 Win 的滚轮(本就修不了) | **推荐 ✅(主线)** |
| **C2 诊断仪表** | 环境变量门控(如 `MYSTERIES_TUI_DEBUG_EVENTS=1`)把原始 crossterm `Event` 落日志,供真机核验滚轮是否到达 | 把「实测收不到」从假设变可验证事实;成本小、纯加法;核心格式化函数可单测 | **推荐 ✅(配套 C1)** |
| **C3 自写 Win32 鼠标输入路径** | Windows 专属用 `ReadConsoleInputW` 直读 `MOUSE_EVENT_RECORD`,绕过 crossterm 解析 | 重、平台分叉、与 crossterm 重复;**且 ConPTY 旧构建本就不往 input buffer 塞鼠标记录 → 绕了也收不到**,不解决平台缺口 | **弃 ❌** |
| **C4 仅文档建议升级 Win11** | 说明平台修复在 conhost ≳ 10.0.22523 | 纯文档、非代码;可作为 C1 的补充说明,但单独不够 | 并入 C1 的提示文案 |

**采用 C1 + C2**:主线靠键盘全覆盖兜底(用户在任何终端都能滚);滚轮保留为尽力而为(新 Win11 / 多数 Unix 终端可用);加诊断开关供真机定位。**不**做 C3(否决理由如上),**不**对 `terminal.rs` 动刀(捕获已正确)。

可发现性(C1,上游拍板:defer):本期不实现键盘滚动提示。待真机冒烟确认滚轮/键盘实际体验后,另议是否在状态行或 notice 增加「↑↓/PgUp·PgDn/Home·End 滚动」提示。

## Risks / Trade-offs

- **折叠默认态改动既有工具卡 insta 快照** → 折叠形态一旦拍板,迁移 `tui_tool_card_done` / `tui_timeline_tool_then_final_answer` / `tui_run_shell_exit_foot` / `tui_permission_state`(含 done 卡)等帧;running 卡折叠态另锁。新增「折叠 vs 展开」两态快照各一帧。属事后 insta,人工对 `设计规范/03` C5 审。
- **`ctrl+o` 与终端/输入冲突** → 在 `Char` arm 前拦截、仅 Press;以单测锁「`ctrl+o` 不进输入框、翻转 `tools_expanded`」。极少数终端可能不投递 `ctrl+o`(被占用),此时折叠仍可由（若上游决定加的）其他绑定触发——本期先只 `ctrl+o`,记账。
- **`↑`/`↓` 将来与输入历史争用** → 已记(决策 ②);本期无输入历史,先判滚动,后续按语境分流再迁移快照。
- **滚轮在旧 Windows 仍不可用** → 这是平台事实,非本 change 能修;C1 键盘兜底确保「不依赖滚轮也能全覆盖滚动」,C2 诊断供定位。**不**承诺滚轮被根治。
- **诊断日志写盘副作用** → 仅环境变量显式开启时写;路径用临时目录/cwd 下固定文件名,失败静默降级不影响主循环;**禁止**记任何凭据(遵 CLAUDE.md)。

## Open Questions(上游拍板结果)

- **A1(折叠单行形态)**:全锁定。折叠行用「`{glyph} {name} {args} · {N} 行 ⌄` / `· exit {code}` / `· 运行中…`」。
- **A2(单条展开)**:全锁定。本期仅全局 `ctrl+o`,不做单条展开。
- **A3(折叠键位)**:全锁定。绑定 `ctrl+o`。
- **B1(方向键归属)**:全锁定。`↑`/`↓` 本期判给 transcript 行级滚动。
- **C1(滚动可发现性提示)**:defer。本期不实现键盘滚动提示,留真机冒烟后另议。
- **C2(诊断开关)**:纳入。本期实现 env `MYSTERIES_TUI_DEBUG_EVENTS` 门控诊断,日志写入 `std::env::temp_dir()` 下固定文件名,失败静默降级、不阻主循环、禁记凭据。
