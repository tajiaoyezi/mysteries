# builtin-tools Delta

## MODIFIED Requirements

### Requirement: run_shell 执行(RequiresConfirmation)

`run_shell` SHALL 经平台 shell(Windows `cmd /C`、Unix `sh -c`)执行命令,捕获 stdout / stderr / exit code;SHALL 受 timeout 约束,超时则终止命令并 → is_error;输出超 `max_output_bytes` 时截断置 `truncated`;非零 exit → is_error;权限级别 `RequiresConfirmation`。

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
