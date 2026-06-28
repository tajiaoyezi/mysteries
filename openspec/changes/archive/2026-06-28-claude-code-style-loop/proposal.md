## Why

8 轮硬上限 + 硬报错是反模式:模型多轮探索工具时极易在「还没准备好回答」就触顶 `max_iterations` → `AgentError::MaxIterations` → TUI 把它落成一个被工具卡盖住、钉底视图看不见的 Error 块,表现为「悄无声息地停了」。本 change 对齐 Claude Code 式交互(无低硬上限、跑到自然结束、用户可随时中断本轮),并修复「最终回答永远被工具卡盖在下面」的可见性缺陷,使一轮对话的产出真正可见、可控。

## What Changes

- **A 跑到自然结束 + 高位安全网 + 触顶强制收尾**:循环仍以「某轮无 tool_calls」自然终止(不变);`DEFAULT_MAX_ITERATIONS` 由 8 抬为 50(高位**安全网**而非常规上限,仍可配);撞到安全网时**不再直接 `Err`**,而是再调一次模型但 `tools` 传空(禁用工具),逼它用现有 history 产出文字回答并返回 `Ok(text)`;仅当这次仍无文字时,才保留 `AgentError::MaxIterations` 作为最后兜底。
- **B 运行中可中断(Esc 停本轮)**:新增 `UserInput::Interrupt`(UI→agent task,**独立中断通道**,不复用 `input_rx` 以免误吞排队 Prompt)与 `AgentEvent::Interrupted`(agent→UI);`run_agent_task` 的 Prompt 分支用 `tokio::select!` 把本轮 `run_observed` 与中断信号并置,中断到达即 drop 本轮 run future(在 `provider.complete` / `tool.execute` 的 await 点协作取消),发 `Interrupted`、回 `Idle`、**程序不退**;Esc 三态分流(等授权=拒绝 / 运行中=中断 / 就绪=退出)。
- **C 结果可见 —— 单一时间线**:`TranscriptBlock` 增 `Tool(ToolCard)` 变体,工具卡按**到达顺序**进入唯一的 transcript Vec(`ToolCallStarted` 时 push、`ToolCallFinished` 按 `id` 回填),删除独立的 `tool_cards` Vec;自然收尾后最终回答成为最后一个块,钉底即可见。工具卡本 change 仍全量展开(折叠留给后续 change)。
- **D 据实纳入(工作树已实现、测试绿,只补 spec delta 不回退)**:① `DEFAULT_SYSTEM_PROMPT` 身份约束(禁止冒充上游模型/厂商,模型名见状态行)② 按键去重(仅 `KeyEventKind::Press`,修 Windows 每键三发)③ transcript 文本换行 + 悬挂缩进 ④ 欢迎屏水平居中 + 垂直留白 ⑤ emoji / 零宽字符宽度度量 ⑥ 输入框光标定位 ⑦ assistant marker `m `→`◆ ` ⑧ 行级滚动 + 鼠标滚轮 + 鼠标捕获。

## Capabilities

### New Capabilities
<!-- 无新增 capability:全部为对既有 agent-loop / tui-shell 的 requirement 修订或追加。 -->

### Modified Capabilities
- `agent-loop`: **MODIFIED**「max_iterations 守卫」—— 由「触顶即 `MaxIterations` 致命终止」改为「高位安全网 + 触顶强制收尾(禁用 tools 逼出文字),仍空才 `MaxIterations` 兜底」;**ADDED**「system prompt 身份约束」(D①)。
- `tui-shell`: **ADDED**「运行中可中断(中断信令 + Esc 三态 + Interrupted 落 transcript)」(B)、「单一时间线 transcript(`TranscriptBlock::Tool`)」(C)、「终端文本排版与宽度度量」(D③④⑤⑥⑦)、「按键事件去重(仅 Press)」(D②);**MODIFIED**「状态行常驻 meta」(`msgs` 排除 Tool 块,C 的连带)、「transcript 滚动」(在 PageUp/PageDown 上增行级 + 鼠标滚轮,D⑧)。

## Impact

- **code**:`src/agent/mod.rs`(loop 收尾语义 + `DEFAULT_SYSTEM_PROMPT`)、`src/config/mod.rs`(`DEFAULT_MAX_ITERATIONS` 8→50)、`src/tui/channel.rs`(`UserInput::Interrupt` / `AgentEvent::Interrupted`)、`src/tui/mod.rs`(中断 `select!` + 独立通道 + 鼠标/按键事件)、`src/tui/app.rs`(`TranscriptBlock::Tool` + 时间线合并 + Esc 三态 + 行级滚动 + 按键去重)、`src/tui/render.rs`(时间线渲染 + 排版/宽度/光标)、`src/tui/terminal.rs`(鼠标捕获)。
- **conversation 不受影响**:Message 归一化(§5.5)的类型与契约不变,无 `conversation` delta。**cli-runtime 不受影响**:中断信令与 `select!` 全在 tui 层,headless `run_cli` 路径不变。
- **设计规范引用 + port/adapt/drop 归类**:
  - 单一时间线(C)= **port**:`设计规范/02-布局与交互` 中对话与工具卡本为**单列时间线**信息流,TUI 之前误分「先文字、后汇总工具卡」两区,本 change port 回单列,修可见性缺陷。
  - Interrupted 态(B)= **adapt**:原型无「中断」态,新增「⊘ 已中断本轮」notice 块(沿用 notice / `info.fg` 语义,属 §9 **非致命**路径,区别于 C7 致命错误框)。
  - 鼠标滚轮 + 行级滚动(D⑧)= **adapt**:`设计规范/02` 的滚动语义在 TUI 既有 port 为 PageUp/PageDown,本 change adapt 增鼠标滚轮 + 行级步进。
  - 欢迎屏居中 + 留白 / 换行 / emoji 宽度 / 光标 / `◆` marker(D③–⑦)= **adapt**:`设计规范/03` C2 欢迎态与 transcript 文本块的终端版式适配(语义保真、非像素一致)。
- **deps**:零新增(`tokio::time`、`crossterm` event/mouse 均已在依赖内)。
