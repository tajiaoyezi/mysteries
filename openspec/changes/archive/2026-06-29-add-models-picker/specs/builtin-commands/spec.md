## ADDED Requirements

### Requirement: /models 命令打开模型 picker

系统 SHALL 提供 `/models` 内置命令(**区别于** `/model [name]`):`parse_command("/models")`(无参)SHALL 归约为 `Command::Models`;执行时打开 TUI 模型 picker 浮层(见 `tui-shell`「模型 picker 浮层」)。`/help` 的命令元数据列表 SHALL 含 `/models`(描述如「浏览 / 切换 provider 与模型」)。`/model [name]`(查看 / 直切当前 provider 的 model)行为 **MUST 不变**。

#### Scenario: 解析 /models 为 Models 命令

- **WHEN** `parse_command("/models")`
- **THEN** 得 `Command::Models`;而 `parse_command("/model claude")` 仍得 `Command::Model(Some("claude"))`、`parse_command("/model")` 仍得 `Command::Model(None)`(二命令并存、不混淆)

#### Scenario: /help 列出 /models

- **WHEN** 取内置命令元数据(`/help` 据此渲染)
- **THEN** 列表含 `/models` 条目(name = `/models`,有描述);`/model` 条目仍在
