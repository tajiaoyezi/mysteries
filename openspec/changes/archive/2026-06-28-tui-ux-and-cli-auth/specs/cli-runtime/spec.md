## ADDED Requirements

### Requirement: auth 子命令交互式配置

系统 SHALL 提供 `mysteries auth` 子命令(由 `main` 分流识别,非 TUI):交互式经 stdin/stderr 依次配置 provider(`openai` / `anthropic`)、base_url(可空 → 用默认 endpoint)、默认 model、API key。API key 输入 MUST **隐藏**(不回显;用既有 `crossterm` raw mode 读取、读毕恢复终端态),key MUST 经 `secrecy::SecretString` 承载、MUST NOT 入日志 / 错误 / 提示输出。配置 SHALL 持久化:provider / base_url / model 经 config 写能力 **merge** 入 user `config.toml`(保留其他字段),API key 经 credential 写能力 **upsert** 入 `credentials`。auth 流程 MUST NOT 触网(仅写配置)。**输入读取 MUST 与流程解耦**(可注入),以便离线确定性单测。读取 EOF / 取消 SHALL 中止且**不写任何文件**(不留半配置)。

#### Scenario: auth 写配置与凭据(注入输入,离线)

- **WHEN** 以注入输入(provider=`openai`、model=`gpt-4o`、key=`sk-xxx`)跑 auth 流程,配置 / 凭据指向临时路径
- **THEN** user `config.toml` 的 `provider.kind=openai`、`model=gpt-4o`(其他字段保留),`credentials` 含 `openai = sk-xxx`;全程不触网

#### Scenario: key 隐藏且不入输出

- **WHEN** auth 读取 API key
- **THEN** 输入不回显;key 经 `SecretString` 承载,任何提示 / 错误输出均不含明文 key

#### Scenario: 中止不留半配置

- **WHEN** auth 中途遇 EOF / 取消
- **THEN** 不写入 `config.toml` 或 `credentials`(既有配置保持原状)
