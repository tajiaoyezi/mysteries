## ADDED Requirements

### Requirement: 工具退出码

`ToolOutcome` SHALL 增 `exit: Option<i32>`:进程类工具(执行外部命令)设其为进程退出码,其余工具 MUST 为 `None`。既有 `content` / `is_error` / `truncated` 字段与其语义 MUST 不变(`exit` 默认 `None`,behavior-preserving)。

#### Scenario: 默认 None,进程类设码

- **WHEN** 构造一个非进程类工具的 `ToolOutcome`
- **THEN** 其 `exit` 为 `None`(既有字段行为不变)
- **WHEN** 进程类工具以退出码 0 结束
- **THEN** 其 `ToolOutcome.exit` 为 `Some(0)`
