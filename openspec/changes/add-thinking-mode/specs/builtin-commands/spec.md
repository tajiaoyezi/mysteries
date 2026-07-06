# builtin-commands Delta

## ADDED Requirements

### Requirement: /think 思考深度命令

系统 SHALL 提供内建命令 `/think`,承担思考的开关与深度调整:`/think`(裸)= 查询当前档并列出可选值(`ThinkArg::Query`,无副作用);`/think off|low|medium|high|xhigh` = 即时设档(`ThinkArg::Set(Depth)`),存活当前会话、MUST NOT 回写配置文件;非法参数 = `ThinkArg::Invalid(String)`,dispatch 时 SHALL 提示合法取值且 MUST NOT 改动当前档。因 `parse_command` 返 `Option<Command>` 无错误通道、`Depth` 为闭合枚举容不下非法串,`/think foo` MUST 落 `Invalid("foo")` 变体(不可复用 `Query`,亦落不到 `Command::Unknown`——`think` 是合法命令名)。`Command`、`BuiltinCommand`、`COMMANDS` 元数据数组、`parse_command` 四处 MUST 同步扩展,硬编码覆盖测试 `command_metadata_covers_all_builtin_commands_and_matches_parser` 的 `expected` 名字列表与 `COMMANDS` 数组长度 MUST 随之更新。

#### Scenario: 裸命令查询

- **WHEN** 解析 `/think`
- **THEN** 得 `Command::Think(ThinkArg::Query)`;分发时展示当前档 + 可选值,不改状态

#### Scenario: 设档即时生效

- **WHEN** 解析 `/think xhigh`
- **THEN** 得 `Command::Think(ThinkArg::Set(Depth::Xhigh))`;分发后共享 depth 变为 `Xhigh`,不回写配置

#### Scenario: 非法参数落 Invalid、提示不改档

- **WHEN** 解析 `/think foo`
- **THEN** 得 `Command::Think(ThinkArg::Invalid("foo"))`;dispatch 出 Notice 列合法取值,当前档保持不变

#### Scenario: 元数据覆盖测试同步

- **WHEN** 运行 `command_metadata_covers_all_builtin_commands_and_matches_parser`
- **THEN** `/think` 在 `expected` 名字列表、`COMMANDS` 长度为 8、断言通过
