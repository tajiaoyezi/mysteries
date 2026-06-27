## ADDED Requirements

### Requirement: run_shell 退出码

`run_shell` SHALL 把进程退出码设入 `ToolOutcome.exit`(= 进程 `ExitStatus` 的 `code()`,被信号终止等无码情形为 `None`);其余 6 个内置工具(`list_dir` / `read_file` / `glob` / `grep` / `write_file` / `edit_file`)的 `ToolOutcome.exit` MUST 为 `None`。`run_shell` 既有 `content` / `is_error` / `truncated` 输出语义 MUST 不变。

#### Scenario: run_shell 设退出码、其余工具 None

- **WHEN** `run_shell` 执行一条退出码为 0 的命令
- **THEN** 其 `ToolOutcome.exit` 为 `Some(0)`,`content` / `is_error` 与既有一致

#### Scenario: 非进程类工具 exit 为 None

- **WHEN** 任一只读 / 写文件工具产出 `ToolOutcome`
- **THEN** 其 `exit` 为 `None`
