# builtin-tools Specification

## Purpose
TBD - created by archiving change add-builtin-tools. Update Purpose after archive.
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

### Requirement: write_file 写入(RequiresConfirmation)

`write_file` SHALL 新建或覆盖写入文件内容;权限级别 `RequiresConfirmation`;写入失败 → is_error。

#### Scenario: 写入新文件

- **WHEN** `write_file` 到 tempdir 内一个新路径
- **THEN** 文件被创建且内容正确,`is_error = false`

#### Scenario: 覆盖既有文件

- **WHEN** `write_file` 到一个已存在的文件
- **THEN** 内容被覆盖

### Requirement: edit_file 唯一匹配替换(RequiresConfirmation)

`edit_file` SHALL 以 str-replace 编辑文件,要求 `old_string` 在文件中**恰好出现一次**;0 次或多于一次匹配 SHALL → is_error 且**不写入**;权限级别 `RequiresConfirmation`。

#### Scenario: 唯一匹配替换

- **WHEN** `edit_file` 的 `old_string` 在文件中恰好出现一次
- **THEN** 该处被替换为 `new_string`,文件更新,`is_error = false`

#### Scenario: 非唯一匹配报错且不改文件

- **WHEN** `old_string` 在文件中出现 0 次或多于一次
- **THEN** `is_error = true`,且文件未被修改

### Requirement: run_shell 执行(RequiresConfirmation)

`run_shell` SHALL 经平台 shell(Windows `cmd /C`、Unix `sh -c`)执行命令,捕获 stdout / stderr / exit code;SHALL 受 timeout 约束,超时则终止命令并 → is_error;输出超 `max_output_bytes` 时截断置 `truncated`;非零 exit → is_error;权限级别 `RequiresConfirmation`。

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

