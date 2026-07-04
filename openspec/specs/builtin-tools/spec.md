# builtin-tools Specification

## Purpose
定义 7 个内置工具的行为契约:4 个只读工具(`list_dir` / `read_file` / `glob` / `grep`,权限级别 `ReadOnly`)与 3 个变更类工具(`write_file` / `edit_file`,`Edit`;`run_shell`,`Execute`),覆盖各自的输入语义、输出截断(`max_output_bytes` / `truncated`)与 exit code 编码。关键立场是失败一律编码为 `ToolOutcome{is_error}` 回给模型而非 panic,变更类工具经权限门 `Deny` 时零副作用,`edit_file` 要求 `old_string` 唯一匹配、否则不写入。工具抽象与注册调度属 tool-system,权限判定机制属 permission-gate;本域仅约定各实体工具自身的行为。
## Requirements
### Requirement: list_dir 列目录(ReadOnly)

`list_dir` SHALL 列出指定目录(默认 `ToolContext.cwd`)下的条目,gitignore 感知(`ignore` crate);权限级别 `ReadOnly`;失败(路径不存在等)SHALL 编码为 `ToolOutcome{is_error: true}`,不 panic。

#### Scenario: 列出目录条目

- **WHEN** 对一个含若干文件的 tempdir 调用 `list_dir`
- **THEN** `ToolOutcome.content` 含这些条目,`is_error = false`

#### Scenario: 路径不存在

- **WHEN** 对一个不存在的路径调用 `list_dir`
- **THEN** 返回 `is_error = true` 的 `ToolOutcome`(不 panic)

### Requirement: read_file 读取与截断(ReadOnly)

`read_file` SHALL 读取文件内容,支持按**行**的 `offset` / `limit` 分页;当内容(分页后)超过 `ToolContext.max_output_bytes`(**字节**,按 UTF-8 字符边界截断)时 SHALL 截断并置 `ToolOutcome.truncated = true`;权限级别 `ReadOnly`;文件不存在 → is_error。

#### Scenario: 读取内容

- **WHEN** `read_file` 一个 tempdir 内已知内容的文件
- **THEN** `content` 等于该内容,`is_error = false`,`truncated = false`

#### Scenario: offset/limit 分页

- **WHEN** `read_file` 带 `offset` / `limit`
- **THEN** 只返回对应区间的内容

#### Scenario: 输出超限截断

- **WHEN** 文件内容超过 `max_output_bytes`
- **THEN** `content` 被截断,`truncated = true`

#### Scenario: 文件不存在

- **WHEN** `read_file` 一个不存在的路径
- **THEN** `is_error = true`

### Requirement: glob 文件匹配(ReadOnly)

`glob` SHALL 用 `globset` 按 pattern 匹配文件路径;权限级别 `ReadOnly`;无效 pattern → is_error。

#### Scenario: 匹配文件

- **WHEN** `glob` 一个匹配 tempdir 内若干文件的 pattern
- **THEN** `content` 列出匹配路径,`is_error = false`

#### Scenario: 无效 pattern

- **WHEN** `glob` 一个非法 pattern
- **THEN** `is_error = true`

### Requirement: grep 内容搜索与截断(ReadOnly)

`grep` SHALL 用 `ignore` 遍历 + `regex` 搜索内容,返回匹配行(含来源定位);输出超 `max_output_bytes` 时 SHALL 截断置 `truncated`;权限级别 `ReadOnly`;无效正则 → is_error。

#### Scenario: 找到匹配

- **WHEN** `grep` 一个在 tempdir 文件中存在的正则
- **THEN** `content` 含匹配行,`is_error = false`

#### Scenario: 无效正则

- **WHEN** `grep` 一个非法正则
- **THEN** `is_error = true`

#### Scenario: 输出超限截断

- **WHEN** 匹配输出超过 `max_output_bytes`
- **THEN** `truncated = true`

### Requirement: write_file 写入(Edit)

`write_file` SHALL 新建或覆盖写入文件内容;权限级别 `Edit`;写入失败 → is_error。

#### Scenario: 写入新文件

- **WHEN** `write_file` 到 tempdir 内一个新路径
- **THEN** 文件被创建且内容正确,`is_error = false`

#### Scenario: 覆盖既有文件

- **WHEN** `write_file` 到一个已存在的文件
- **THEN** 内容被覆盖

### Requirement: edit_file 唯一匹配替换(Edit)

`edit_file` SHALL 以 str-replace 编辑文件,要求 `old_string` 在文件中**恰好出现一次**;0 次或多于一次匹配 SHALL → is_error 且**不写入**;权限级别 `Edit`。

#### Scenario: 唯一匹配替换

- **WHEN** `edit_file` 的 `old_string` 在文件中恰好出现一次
- **THEN** 该处被替换为 `new_string`,文件更新,`is_error = false`

#### Scenario: 非唯一匹配报错且不改文件

- **WHEN** `old_string` 在文件中出现 0 次或多于一次
- **THEN** `is_error = true`,且文件未被修改

### Requirement: run_shell 执行(Execute)

`run_shell` SHALL 经平台 shell(Windows `cmd /C`、Unix `sh -c`)执行命令,捕获 stdout / stderr / exit code;SHALL 受 timeout 约束,超时则终止命令并 → is_error;输出超 `max_output_bytes` 时截断置 `truncated`;非零 exit → is_error;权限级别 `Execute`。

**console 独立性(Windows)**:子进程 SHALL 以 `CREATE_NO_WINDOW`(`0x0800_0000`,具名常量、`#[cfg(windows)]`)创建——不 attach 调用方 console,防止子进程重置 TUI 已设置的终端输入模式(`ENABLE_MOUSE_INPUT` 等,重置后终端把滚轮降级为方向键);stdout / stderr 经 pipe 捕获,MUST 不受该标志影响。非 Windows 平台无此问题,不加标志。

#### Scenario: 捕获输出与 exit

- **WHEN** `run_shell` 一个打印已知文本并成功退出的命令
- **THEN** `content` 含该输出与 exit code,`is_error = false`

#### Scenario: 超时终止

- **WHEN** `run_shell` 一个超过 timeout 仍未结束的命令
- **THEN** 命令被终止,`is_error = true`

#### Scenario: 非零退出

- **WHEN** 命令以非零 exit code 结束
- **THEN** `is_error = true`,`content` 含输出与 exit code

### Requirement: 变更工具经权限门拒绝时无副作用

经 Agent loop 调用的变更工具,在权限门返回 `Deny` 时 SHALL NOT 产生副作用(文件不被创建 / 修改、命令不被执行),且 is_error 的 `ToolResult` 入 history(由 `permission-gate` 保证;此处验证实体工具确受其约束)。

#### Scenario: 拒绝 write_file 无副作用

- **WHEN** Agent loop(注入 `DenyAll` decider)处理一个 `write_file` 的 tool_call
- **THEN** 目标文件未被创建,history 含一条 is_error 的 `ToolResult`

### Requirement: run_shell 退出码

`run_shell` SHALL 把进程退出码设入 `ToolOutcome.exit`(= 进程 `ExitStatus` 的 `code()`,被信号终止等无码情形为 `None`);其余 6 个内置工具(`list_dir` / `read_file` / `glob` / `grep` / `write_file` / `edit_file`)的 `ToolOutcome.exit` MUST 为 `None`。`run_shell` 既有 `content` / `is_error` / `truncated` 输出语义 MUST 不变。

#### Scenario: run_shell 设退出码、其余工具 None

- **WHEN** `run_shell` 执行一条退出码为 0 的命令
- **THEN** 其 `ToolOutcome.exit` 为 `Some(0)`,`content` / `is_error` 与既有一致

#### Scenario: 非进程类工具 exit 为 None

- **WHEN** 任一只读 / 写文件工具产出 `ToolOutcome`
- **THEN** 其 `exit` 为 `None`

