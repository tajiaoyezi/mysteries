## MODIFIED Requirements

### Requirement: slash 命令解析

系统 SHALL 提供纯函数 `parse_command(input: &str) -> Option<Command>`:输入以 `/` 起头时解析为 `Command`(`Help` / `Clear` / `Model(Option<String>)` / `Status` / `Exit` / `Login` / `Logout` / `Compact` / `Unknown(String)`),否则 `None`(当普通 prompt)。解析 MUST 不触发任何副作用(IO / 网络),可离线单测。系统 SHALL 额外提供内置命令**元数据**清单(每项:命令名 / 简述 / 用法),供 `/` 命令补全 UI 列出与过滤;元数据清单与 `parse_command` 可识别的命令集 MUST **同源**(单一定义,避免补全与解析漂移)。

#### Scenario: 识别各内置命令

- **WHEN** 解析 `"/help"` / `"/model gpt-4o"` / `"/clear"` / `"/xyz"`
- **THEN** 分别得 `Command::Help` / `Command::Model(Some("gpt-4o"))` / `Command::Clear` / `Command::Unknown("xyz")`;非 `/` 开头 → `None`

#### Scenario: 命令元数据可供补全且与解析同源

- **WHEN** 取内置命令元数据清单
- **THEN** 清单含各内置命令的 名 / 简述 / 用法,且其命令集与 `parse_command` 可识别的内置命令集一致(同源,无遗漏 / 无多余)
