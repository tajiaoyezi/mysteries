# builtin-commands Specification

## Purpose
TBD - created by archiving change finish-1-0. Update Purpose after archive.
## Requirements
### Requirement: slash 命令解析

系统 SHALL 提供纯函数 `parse_command(input: &str) -> Option<Command>`:输入以 `/` 起头时解析为 `Command`(`Help` / `Clear` / `Model(Option<String>)` / `Status` / `Exit` / `Login` / `Logout` / `Unknown(String)`),否则 `None`(当普通 prompt)。解析 MUST 不触发任何副作用(IO / 网络),可离线单测。

#### Scenario: 识别各内置命令

- **WHEN** 解析 `"/help"` / `"/model gpt-4o"` / `"/clear"` / `"/xyz"`
- **THEN** 分别得 `Command::Help` / `Command::Model(Some("gpt-4o"))` / `Command::Clear` / `Command::Unknown("xyz")`;非 `/` 开头 → `None`

### Requirement: 命令执行语义

系统 SHALL 在提交输入时:非 `/` → 当 prompt 发给 agent-task;`/` 命令按语义执行 —— `Clear` 清空 transcript;`Help` → 追加 C8 帮助块(7 命令);`Status` → 追加 C9 快照块;`Exit` → 退出 TUI;`Login`/`Logout` → 追加占位 notice(提示用 config / 环境变量配置凭据);`Unknown` → 追加「未知命令」notice;`Model` 见专项 requirement。命令执行 MUST 可单测(状态变更断言,无终端)。

#### Scenario: /clear 清空、/help 追加帮助块

- **WHEN** transcript 非空时执行 `Clear`,随后执行 `Help`
- **THEN** transcript 先被清空,再含一个 C8 帮助块(列出 7 命令)

#### Scenario: 占位与未知命令

- **WHEN** 执行 `Login` / `Logout` / `Unknown("x")`
- **THEN** 各追加一条 notice(占位提示 / 未知命令),不影响 agent-task

### Requirement: /model 查看与运行时切换

`/model`(无参)SHALL 追加一条显示**当前 model** 的 notice;`/model <name>` SHALL 乐观更新 UI 的当前 model 并经 `UserInput::SetModel(name)` 通知 agent-task,使**后续轮**用新 model(当前进行中的轮不受影响)。切换 MUST NOT 破坏既有 `run` / `run_observed` 行为。

#### Scenario: 查看当前 model

- **WHEN** 当前 model 为 `"m1"`,执行 `Model(None)`
- **THEN** 追加一条 notice 显示 `"m1"`

#### Scenario: 切换 model 影响后续轮

- **WHEN** 执行 `Model(Some("m2"))`
- **THEN** UI 当前 model 显示 `"m2"`,并向 agent-task 发出 `SetModel("m2")`;下一轮 `ModelRequest.model` 为 `"m2"`

### Requirement: /compact 手动压缩

`/compact` 命令 SHALL 立即对当前会话 history 跑一次压缩(**无视阈值**,直接压),复用与自动压缩**同一** `Compacting` 逻辑(被压区间 / 结构化 summary / 入 `System` / 保留窗口与正确性红线一致)。压缩结果替换会话 history,并回一条 notice(含压缩前后消息数);summary 失败时 SHALL 回 notice 提示可重试(history 不变),MUST NOT panic。压缩禁用(未配 `model_context_window`)或无 provider 时,`/compact` SHALL 回提示而非压缩、MUST NOT panic。命令解析与执行走既有 builtin-commands 语义(同 `/model` 等)。

#### Scenario: /compact 立即压缩

- **WHEN** 在配了 `model_context_window` 的会话中输入 `/compact`(Mock provider 返回 summary)
- **THEN** 当前 history 被替换为 `[ System(原 system + summary), 最近 keep_recent_turns 轮 ]`,回一条 notice 含压缩前后消息数

#### Scenario: /compact summary 失败回 notice

- **WHEN** 输入 `/compact` 但 summary 的 `provider.complete` 失败
- **THEN** history 保持不变,回一条 notice 提示压缩失败 / 可重试,不 panic

#### Scenario: 压缩禁用时 /compact 回提示

- **WHEN** 未配 `model_context_window` 时输入 `/compact`
- **THEN** 回一条提示(压缩未启用 / 需配 `model_context_window`),history 不变、不 panic

