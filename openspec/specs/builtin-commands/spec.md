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

