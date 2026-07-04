# builtin-tools Delta

## MODIFIED Requirements

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
