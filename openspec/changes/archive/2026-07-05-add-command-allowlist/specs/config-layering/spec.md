# config-layering Delta

## ADDED Requirements

### Requirement: 命令 allowlist 配置项(allowed_commands 并集 merge)

`RawConfig` SHALL 支持可选 `allowed_commands: Vec<String>`(`#[serde(default)]`);解析为运行配置时 `Config.allowed_commands: Vec<String>`(缺省为空 `Vec`)。user 层与 project 层 merge 时 `allowed_commands` MUST 取**并集**(dedup),而非字段级 override——命令白名单为**信任集合**、两层叠加(project 可预置仓库级白名单,user 累积 always-allow)。此为对本域「字段级 merge(project 覆盖 user)」通则的**显式集合语义例外**。系统 SHALL 提供 `append_allowed_command(path, cmd)`:对 user config 做 read-modify-write、去重加入、序列化回 TOML(仿「配置写入(merge 持久化)」),供 always-allow 落盘复用。

#### Scenario: allowed_commands 并集 merge

- **WHEN** user `allowed_commands = ["git status"]`、project `allowed_commands = ["cargo build"]`,对二者 merge
- **THEN** 结果为 `["cargo build", "git status"]`(并集、dedup、有序),而非 project 覆盖 user

#### Scenario: 缺省为空 Vec

- **WHEN** user 与 project 均未设 `allowed_commands`
- **THEN** `Config.allowed_commands` 为空 `Vec`

#### Scenario: append 去重落盘

- **WHEN** 对已含 `"git status"` 的 config 先 `append_allowed_command("git status")` 再 `append_allowed_command("ls")`
- **THEN** 落盘后 `allowed_commands` 为 `["git status", "ls"]`(原项不重复),可再读回
