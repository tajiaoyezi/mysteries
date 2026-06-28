## ADDED Requirements

### Requirement: 配置写入(merge 持久化)

系统 SHALL 提供把部分字段 **merge** 写入 user `config.toml` 的能力(read-modify-write):读现有 `config.toml`(不存在则当空)→ 覆盖指定字段(如 `provider.kind` / `provider.base_url` / `model`)→ **保留所有其他字段**后序列化回写。MUST NOT 整文件覆盖而丢失用户既有配置(如 `max_iterations` / `model_context_window` / `compact_trigger_ratio` 等)。路径由调用方注入以便临时文件测试;写入失败 SHALL 返回错误(不静默)。

#### Scenario: merge 写保留其他字段

- **WHEN** `config.toml` 含 `max_iterations = 40` 与 `model = "old"`,对 `model` 写入 `"new"`
- **THEN** 回写后 `model = "new"` 且 `max_iterations = 40` 仍在(其他字段未丢失)

#### Scenario: 文件不存在则新建

- **WHEN** user `config.toml` 不存在时写入 `model = "m"`
- **THEN** 新建该文件并含 `model = "m"`,不报「文件缺失」错误
