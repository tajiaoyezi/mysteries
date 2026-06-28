## Context

现状(已对代码核实):

- `src/agent/mod.rs` 循环 `for _ in 0..max_iterations { ... }`,跑满未自然终止即 `Err(AgentError::MaxIterations)`;`DEFAULT_MAX_ITERATIONS = 8`(`src/config/mod.rs`)。
- 模型多轮探索工具时,8 轮极易在尚未给出文字答复时触顶 → `run_agent_task` 把 `AgentError` 转 `AgentEvent::Error` → `AppState` push 一个 `TranscriptBlock::Error`。
- 渲染上,`render.rs` 先渲 transcript 文本块、再把 `state.tool_cards` 内所有工具卡渲在末尾,且视图钉底 → 最终回答 / Error 块永远在工具卡上方、被钉底视口盖住 → 表现为「悄无声息地停了」。
- `UserInput` 仅 `Prompt` / `SetModel`,`AgentEvent` 无中断变体;`run_agent_task` 的 Prompt 分支顺序 `await` 整轮 `run_observed`,期间不可中断。

行业参照:Claude Code 交互模式无低硬上限,靠跑到自然结束 + 用户中断;Codex(上限~60)/ OpenCode 触顶时改发「让模型总结收尾」提示而非报错。本设计对齐该范式。

权威次序:行为以 code / 测试为准;D 的 8 项已在工作树实现且测试绿,本 change 据实补 spec,不回退、不重写。

## Goals / Non-Goals

**Goals:**
- 循环跑到自然结束;高位安全网取代低硬上限;触顶**强制收尾**(禁用 tools 逼出文字)而非硬报错。
- 本轮运行中可经 Esc 协作中断,回 Idle、程序不退。
- 工具卡与对话合并为**单一时间线**,最终回答钉底即见。
- D 的 8 项据实落 spec(身份约束 / 按键去重 / 排版 / 宽度度量 / 光标 / 滚动)。

**Non-Goals(本 change 明确不做):**
- 工具输出折叠 + `ctrl+o` 全局展开(留 `improve-tui-interaction`)。
- `↑` / `↓` / `Home` / `End` 滚动键位补全(本 change 仅 PageUp/PageDown + 鼠标滚轮 + 行级方法)。
- 取消时对半写文件的事务化回滚(见 Risks,v1.x 接受现状)。
- 持久化 / token 压缩 / 多 provider 并发等 1.x 路线项。

## Decisions

### ① 安全网值与强制收尾的语义、返回契约

- **安全网值 = 50**(`DEFAULT_MAX_ITERATIONS: 8 → 50`)。理由:50 轮足以覆盖绝大多数「多步探索后收尾」的真实任务(对标 Codex ~60 同量级),又能在模型陷入工具死循环时及时止损;`config-layering` spec 只规定「未设时套用文档化默认常量」、未钉具体值,故抬值**不需** config spec 变更,仍经配置可覆盖。
- **强制收尾语义**:循环结构改为「跑 `max_iterations` 轮;每轮若无 tool_calls 即自然返回 `Ok(text)`」。**当且仅当**跑满 `max_iterations` 轮仍未自然终止时,**追加一次** `provider.complete`,但该次 `ModelRequest.tools` 传**空**(禁用工具),强制模型基于现有 history 产出文字。
- **返回契约**:
  - 自然终止:`Ok(text)`(不变)。
  - 触顶强制收尾且该次有文字:`Ok(text)`,该 `Assistant{text}` 入 history。
  - 触顶强制收尾仍无文字(空 text 且无 tool_calls,或该次又返回 tool_calls 但已被禁用而忽略):兜底 `Err(AgentError::MaxIterations)`。
  - 强制收尾那次 `provider.complete` 自身 `Err` → 按既有「provider 错误致命」分流为 `Err(AgentError::Provider)`。
- **备选(弃)**:触顶直接 `Err`(现状,反模式);触顶静默截断不告知(掩盖问题)。选「禁用 tools 再逼一次」因其既给出可用答复、又保留 `MaxIterations` 作真·失控兜底。

### ② 中断的并发模型(通道选型 / 取消点 / 取消安全性)

- **通道选型**:新增**独立**中断通道 `mpsc::UnboundedReceiver<Interrupt>`(或 `tokio::sync::Notify`),**不复用** `input_rx`。理由:若用 `input_rx` 去 `select!`,会在中断臂里**误吞**排队中的 `Prompt`/`SetModel`。UI 端 Esc(运行中)经新 `UserInput::Interrupt` 投入,`run_agent_task` 把它路由到独立中断信号。
- **取消机制**:`run_agent_task` 的 Prompt 分支用 `tokio::select!`,A 臂 = 本轮 `agent.run_observed(...)`,B 臂 = 中断信号。中断先到则 `select!` 直接 **drop** 掉 A 臂的 run future —— Rust async **协作取消**:future 在下一个 `.await`(`provider.complete` 的 HTTP await / `tool.execute` 的 await)处停在原地、被 drop。随后发 `AgentEvent::Interrupted`、状态回 `Idle`。
- **取消点 / 取消安全性边界**:
  - 卡在 `provider.complete`(reqwest HTTP)被 drop → in-flight 请求随之取消,**安全**(无副作用)。
  - 卡在只读工具(`read_file`/`glob`/`grep`/`list_dir`)被 drop → **安全**(无写)。
  - 卡在 `write_file` / `edit_file` / `run_shell` 的 `execute` 被 drop → **可能留下半写文件 / 半跑命令**。本 change **接受**此边界(v1.x):写工具单次写通常很快、且都过权限门(用户已确认),中断撞上写窗口的概率与代价低。**不**引入 kill-on-drop / 写事务化(记入 Non-Goals;若实测有痛点,后续 change 再加「写操作不可中断段」或临时文件 + 原子 rename)。
- **状态不变量**:中断只结束**本轮**;agent task 循环回到 recv 下一个 `UserInput`,程序存活。中断到达后**不得**再次调用 provider(以「provider 未被再次调用」断言锁死)。
- **备选(弃)**:`AbortHandle` / 显式 cancel token 透传到每个 await —— 对当前规模过重;`select! + drop` 已满足协作取消且零侵入 agent-loop。

### ③ 时间线合并方案(`TranscriptBlock::Tool` vs 备选)

- **选定**:`TranscriptBlock` 增 `Tool(ToolCard)` 变体;删除独立 `tool_cards: Vec<ToolCard>`,工具卡按**到达顺序**进入唯一 `transcript: Vec<TranscriptBlock>`。`ToolCallStarted` 时 push 一个 `Tool(ToolCard{status: running})`;`ToolCallFinished` 时按 `id` 在 transcript 中**回填**对应卡(置 done/error + output + exit)。`render.rs` 顺序遍历 transcript 渲染各块(文本块按文本渲、Tool 块按 C5 渲),**不再**末尾汇总工具卡。
- **为何选它**:这是把信息流 port 回 `设计规范/02` 的**单列时间线**(对话与工具活动本就同列、按时间排)。最终回答自然成为 transcript 最后一个块,钉底即见 —— 从根上修「被工具卡盖住」缺陷,且渲染逻辑更简单(一条 Vec、一趟遍历)。
- **`msgs` 连带**:状态行 meta 的 `msgs` 原定义为「transcript 块数」。合并后 transcript 含 Tool 块,若直接计数会把工具活动算进消息数。故 **MODIFIED**「状态行常驻 meta」:`msgs` = transcript 中**对话块(User/Assistant)**数,**不含** `Tool` 块(也不含 Help/Status/Notice 命令块),保持「消息数」语义。
- **备选(弃)**:
  - 保留两 Vec、渲染时按时间戳归并 —— 需给每块加时间戳并每帧排序,复杂且易错。
  - 工具卡内联进**所属 Assistant 块** —— 与「一卡一到达顺序」不符,且 backfill 定位更绕。
  - 单 Vec 选定方案以最小数据模型改动达成单列时间线。

### ④ Esc 三态决策表

`on_key` / `should_exit` 据 `AppState` 当前态对 Esc 分流(三态互斥,按优先级自上而下):

| 优先级 | 条件(AppState) | Esc 行为 | 信令 | 是否退出 |
| --- | --- | --- | --- | --- |
| 1 | `pending_permission.is_some()` | **拒绝**当前授权(不变) | 经 oneshot 回 `Deny` | 否 |
| 2 | 无 pending 且**本轮运行中**(phase ∈ {CallingModel, ExecutingTool, …} / 非 Idle) | **中断**本轮(新) | `UserInput::Interrupt` | 否 |
| 3 | 无 pending 且**就绪**(phase = Idle / Ready) | **退出**程序(不变) | `should_exit → true` | 是 |

- 仅 `KeyEventKind::Press` 触发(D② 去重);Release/Repeat 直接忽略,避免 Windows 每键三发导致误中断 / 误退。
- 运行态判定取 `AppState` 既有 phase 字段,无需新增「是否运行中」镜像状态。

### 视觉条目与偏差归类(供评审核对)

- 单列时间线 = **port**(`设计规范/02`)。
- `⊘ 已中断本轮` notice = **adapt**(原型无中断态;沿用 notice / `info.fg`,§9 非致命,区别 C7 致命框)。
- 鼠标滚轮 + 行级滚动 = **adapt**(`设计规范/02` 滚动语义的 TUI 增补)。
- 欢迎屏居中 / 留白 / 换行 / emoji 宽度 / 光标 / `◆` marker = **adapt**(`设计规范/03` C2 / transcript 终端版式,语义保真非像素一致)。

## Risks / Trade-offs

- **写工具中断留半写文件** → 接受(v1.x);写过权限门、单次写快、撞窗口概率低。后续可加「写段不可中断」或临时文件 + 原子 rename。Non-Goal 已记录。
- **强制收尾那次模型仍只想调工具** → tools 传空使其无工具可调,多数会转文字;仍空则 `MaxIterations` 兜底,不会静默卡死。
- **安全网 50 偏高致失控任务多烧 token** → 触顶有强制收尾止损;且可经配置下调。相比「8 轮误杀正常任务」,误判代价更小。
- **单 Vec 回填按 id 定位** → `ToolCallFinished` 若找不到匹配 id(异常)应安全降级(忽略或追加),不 panic;以测试覆盖「Finished 先于/无 Started」边界。
- **中断与排队 Prompt 竞争** → 独立通道根除「误吞 Prompt」;以「中断不消费 input_rx 里的 Prompt」测试锁死。
- **据实纳入使本 change 偏宽** → 已与用户确认(Option A:spec==code、不拆 hunk);`improve-tui-interaction` 仍保留(折叠 + ctrl+o + ↑↓/Home/End)。
