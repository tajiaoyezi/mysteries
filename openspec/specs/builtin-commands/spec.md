# builtin-commands Specification

## Purpose
定义会话内 slash 命令的解析与执行语义:纯函数 `parse_command` 把 `/` 起头的输入归约为 `Command`,并约定各内置命令(`/help` / `/clear` / `/model` / `/models` / `/status` / `/compact` / `/exit`)对 transcript 与会话状态的效果。关键立场是解析零副作用、命令元数据与可解析命令集单一同源(`/` 补全 UI 直接取用,防补全与执行漂移)。补全浮层与模型 picker 的呈现交互属 tui-shell,`/compact` 复用的 `Compacting` 压缩逻辑属 context-strategy;auth 类操作已迁至 CLI `mysteries auth`(cli-runtime),不在本命令集内。
## Requirements
### Requirement: slash 命令解析

系统 SHALL 提供纯函数 `parse_command(input: &str) -> Option<Command>`:输入以 `/` 起头时解析为 `Command`(`Help` / `Clear` / `Model(Option<String>)` / `Status` / `Exit` / `Compact` / `Unknown(String)`),否则 `None`(当普通 prompt)。解析 MUST 不触发任何副作用(IO / 网络),可离线单测。系统 SHALL 额外提供内置命令**元数据**清单(每项:命令名 / 简述 / 用法),供 `/` 命令补全 UI 列出与过滤;元数据清单与 `parse_command` 可识别的命令集 MUST **同源**(单一定义,避免补全与解析漂移)。**`/login` / `/logout` 不再是内置命令**(auth 迁至 CLI `mysteries auth`),MUST 解析为 `Unknown`(`/login` → `Unknown("login")`、`/logout` → `Unknown("logout")`),且 MUST NOT 出现在内置命令元数据清单中。

#### Scenario: 识别各内置命令

- **WHEN** 解析 `"/help"` / `"/model gpt-4o"` / `"/clear"` / `"/login"` / `"/xyz"`
- **THEN** 分别得 `Command::Help` / `Command::Model(Some("gpt-4o"))` / `Command::Clear` / `Command::Unknown("login")`(`/login` 已非内置)/ `Command::Unknown("xyz")`;非 `/` 开头 → `None`

#### Scenario: 命令元数据可供补全且与解析同源

- **WHEN** 取内置命令元数据清单
- **THEN** 清单含各内置命令的 名 / 简述 / 用法(`/help` `/clear` `/model` `/status` `/exit` `/compact`,不含 `/login` `/logout`),且其命令集与 `parse_command` 可识别的内置命令集一致(同源,无遗漏 / 无多余)

### Requirement: 命令执行语义

系统 SHALL 在提交输入时:非 `/` → 当 prompt 发给 agent-task;`/` 命令按语义执行 —— `Clear` 清空 transcript;`Help` → 追加 C8 帮助块(6 个帮助条目);`Status` → 追加 C9 快照块;`Exit` → 退出 TUI;`Unknown` → 追加「未知命令」notice(形如 `未知命令: /{name}`;**已删除的 `/login` / `/logout` 经此路径**,不再有专门占位 notice);`Model` 见专项 requirement。命令执行 MUST 可单测(状态变更断言,无终端)。

#### Scenario: /clear 清空、/help 追加帮助块

- **WHEN** transcript 非空时执行 `Clear`,随后执行 `Help`
- **THEN** transcript 先被清空,再含一个 C8 帮助块(列出 6 个帮助条目)

#### Scenario: 未知命令(含已删的 /login /logout)

- **WHEN** 执行 `Unknown("login")` / `Unknown("logout")` / `Unknown("x")`
- **THEN** 各追加一条「未知命令」notice(`未知命令: /login` / `未知命令: /logout` / `未知命令: /x`),不影响 agent-task

### Requirement: /model 查看与运行时切换

`/model`(无参)SHALL 追加一条显示**当前 model** 的 notice,**且含切换引导**(形如 `当前 model: {model} — 输入 /model <name> 切换`,提示无参仅查看、带参可切换);`/model <name>` SHALL 乐观更新 UI 的当前 model 并经 `UserInput::SetModel(name)` 通知 agent-task,使**后续轮**用新 model(当前进行中的轮不受影响)。切换 MUST NOT 破坏既有 `run` / `run_observed` 行为。

#### Scenario: 查看当前 model

- **WHEN** 当前 model 为 `"m1"`,执行 `Model(None)`
- **THEN** 追加一条 notice 显示 `"m1"`,**且含切换引导 `/model <name>`**(提示无参仅查看)

#### Scenario: 切换 model 影响后续轮

- **WHEN** 执行 `Model(Some("m2"))`
- **THEN** UI 当前 model 显示 `"m2"`,并向 agent-task 发出 `SetModel("m2")`;下一轮 `ModelRequest.model` 为 `"m2"`

### Requirement: /compact 手动压缩

`/compact` 命令 SHALL 立即对当前会话 history 跑一次压缩(**无视阈值**,直接压),复用与自动压缩**同一** `Compacting` 逻辑(被压区间 / 结构化 summary / 入 `System` / 保留窗口与正确性红线一致)。压缩默认启用(有效窗口按「上下文窗口解析」求得,见 context-strategy),MUST NOT 出现「压缩未启用」类提示,`run_compact_command` MUST 以非 `Option` 的压缩句柄执行(「未启用」分支删除)。

**发起门控**:`/compact` 仅在 `phase == Ready` 且无排队时可发起——发起即置 `Phase::Compacting` 并 send `UserInput::Compact`;运行中 / 有排队时 SHALL 拒绝并回 notice(如「当前有任务进行中,/compact 请稍后再试」),MUST NOT 把 `Compact` 排进 channel 延迟执行。

**进行中(动画与排队)**:`Compacting` 期间 activity line SHALL 以 spinner 显示「压缩上下文…」(accent 样式,**不**提示 esc 中断——压缩不可中断,v1 Non-Goal);phase 非 Ready,期间提交按「消息排队」(tui-shell)语义入可见队列。

**收场**:压缩结束(成功或失败)agent task SHALL 依次发 `Notice(结果文案)` 与 `AgentEvent::CompactDone`;`apply(CompactDone)` 置 `phase = Ready`,且 `CompactDone` 计入排队推进闸门(见 tui-shell「消息排队」)。成功 notice MUST 为**不含消息数**的简短文案(「已压缩上下文」);summary 失败时 SHALL 回 notice 提示可重试(history 不变),MUST NOT panic。命令解析与执行走既有 builtin-commands 语义(同 `/model` 等)。

#### Scenario: /compact 立即压缩且 notice 不含计数

- **WHEN** 就绪且无排队时输入 `/compact`(Mock provider 返回 summary)
- **THEN** 当前 history 被替换为 `[ System(原 system + summary), 最近 keep_recent_turns 轮 ]`;成功 notice 为「已压缩上下文」(**不含**前后消息数);随后 `CompactDone` 置回 Ready

#### Scenario: /compact summary 失败回 notice(仍收场)

- **WHEN** 输入 `/compact` 但 summary 的 `provider.complete` 失败
- **THEN** history 保持不变,回一条 notice 提示压缩失败 / 可重试,不 panic;**仍发 `CompactDone`** 置回 Ready(不卡在 Compacting)

#### Scenario: 未配 model_context_window 时 /compact 仍可压

- **WHEN** 未配 `model_context_window` 时输入 `/compact`(Mock provider 返回 summary)
- **THEN** 照常压缩(手动压无视阈值,与窗口无关),全程不出现「压缩未启用」类提示、不 panic

#### Scenario: 运行中 / 有排队时 /compact 被拒

- **WHEN** `phase` 运行中(如 `CallingModel`),或 `phase == Ready` 但 `pending_queue` 非空,输入 `/compact`
- **THEN** **不** send `UserInput::Compact`、phase 不变,transcript 追加一条「稍后再试」notice

#### Scenario: 压缩进行中活动行动画(insta 快照)

- **WHEN** `phase == Compacting`,`TestBackend` 渲染
- **THEN** activity line 为 spinner + 「压缩上下文…」(accent 样式、**无** esc 中断提示),与锁定快照一致

### Requirement: /models 命令打开模型 picker

系统 SHALL 提供 `/models` 内置命令(**区别于** `/model [name]`):`parse_command("/models")`(无参)SHALL 归约为 `Command::Models`;执行时打开 TUI 模型 picker 浮层(见 `tui-shell`「模型 picker 浮层」)。`/help` 的命令元数据列表 SHALL 含 `/models`(描述如「浏览 / 切换 provider 与模型」)。`/model [name]`(查看 / 直切当前 provider 的 model)行为 **MUST 不变**。

#### Scenario: 解析 /models 为 Models 命令

- **WHEN** `parse_command("/models")`
- **THEN** 得 `Command::Models`;而 `parse_command("/model claude")` 仍得 `Command::Model(Some("claude"))`、`parse_command("/model")` 仍得 `Command::Model(None)`(二命令并存、不混淆)

#### Scenario: /help 列出 /models

- **WHEN** 取内置命令元数据(`/help` 据此渲染)
- **THEN** 列表含 `/models` 条目(name = `/models`,有描述);`/model` 条目仍在

### Requirement: /think 思考深度命令

系统 SHALL 提供内建命令 `/think`,承担思考的开关与深度调整:`/think`(裸)= 查询当前档并列出可选值(`ThinkArg::Query`,无副作用);`/think off|low|medium|high|xhigh` = 即时设档(`ThinkArg::Set(Depth)`),存活当前会话、MUST NOT 回写配置文件;非法参数 = `ThinkArg::Invalid(String)`,dispatch 时 SHALL 提示合法取值且 MUST NOT 改动当前档。因 `parse_command` 返 `Option<Command>` 无错误通道、`Depth` 为闭合枚举容不下非法串,`/think foo` MUST 落 `Invalid("foo")` 变体(不可复用 `Query`,亦落不到 `Command::Unknown`——`think` 是合法命令名)。`Command`、`BuiltinCommand`、`COMMANDS` 元数据数组、`parse_command` 四处 MUST 同步扩展,硬编码覆盖测试 `command_metadata_covers_all_builtin_commands_and_matches_parser` 的 `expected` 名字列表与 `COMMANDS` 数组长度 MUST 随之更新。

#### Scenario: 裸命令查询

- **WHEN** 解析 `/think`
- **THEN** 得 `Command::Think(ThinkArg::Query)`;分发时展示当前档 + 可选值,不改状态

#### Scenario: 设档即时生效

- **WHEN** 解析 `/think xhigh`
- **THEN** 得 `Command::Think(ThinkArg::Set(Depth::Xhigh))`;分发后共享 depth 变为 `Xhigh`,不回写配置

#### Scenario: 非法参数落 Invalid、提示不改档

- **WHEN** 解析 `/think foo`
- **THEN** 得 `Command::Think(ThinkArg::Invalid("foo"))`;dispatch 出 Notice 列合法取值,当前档保持不变

#### Scenario: 元数据覆盖测试同步

- **WHEN** 运行 `command_metadata_covers_all_builtin_commands_and_matches_parser`
- **THEN** `/think` 在 `expected` 名字列表、`COMMANDS` 长度为 8、断言通过

