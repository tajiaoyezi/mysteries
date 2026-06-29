## ADDED Requirements

### Requirement: 首次运行 TUI 自动引导

系统 SHALL 提供 `load_config_or_onboard(paths, prompter: &mut dyn AuthPrompter) -> Result<Config, CliError>`:当 user 与 project **两份配置文件皆不存在**(判定经纯函数 `is_first_run(paths)`,即「干净零配置 = 首次运行」)时,先经注入的 `AuthPrompter` 运行既有 `auth login` 引导(写 user `config.toml` + `credentials`),再 `load_config` 返回 `Config`;否则(**任一文件存在**)MUST **跳过引导**、直接 `load_config`。TUI 入口(`run_tui`)MUST 在进入 ratatui 终端态(`TerminalGuard`)**之前**调用本函数,使引导走普通 stdin / raw-mode 提示、不与 TUI 持久 raw mode 冲突。引导中途取消(`AuthError::Cancelled`)MUST 冒泡为 `Err`,且 **不写半份配置**(沿用 `run_auth_login` 写文件前取消的既有保证)。引导触发 / 跳过 / 取消逻辑 MUST 可经 mock `AuthPrompter` + 临时目录**离线确定性**单测;本函数 MUST NOT 触网。

#### Scenario: 首启零配置触发引导并写配置(注入,离线)

- **WHEN** user 与 project 两路径(临时目录)皆不存在,以脚本化 `AuthPrompter`(选 `OpenAI` 预设、填 key)调用 `load_config_or_onboard`
- **THEN** 返回 `Ok(Config)`(`model` = OpenAI 预设默认 model 常量、`provider.kind = OpenAi`),且 user `config.toml` 已生成、`credentials` 含 `openai` 条目;全程不触网

#### Scenario: 已有配置则跳过引导(prompter 不被调用)

- **WHEN** user 路径已存在合法 `config.toml`(含 `model` 与 `provider.kind`),以「一旦被调用即 panic」的 mock `AuthPrompter` 调用 `load_config_or_onboard`
- **THEN** 返回 `Ok(Config)`(取自既有配置),`AuthPrompter` 全程未被调用(引导被跳过)

#### Scenario: 文件存在但残缺仍照旧报错(不引导、不覆盖)

- **WHEN** 某配置文件存在但残缺(如有 `model` 无 `provider.kind`,或 TOML 非法),以「一旦被调用即 panic」的 mock `AuthPrompter` 调用 `load_config_or_onboard`
- **THEN** 跳过引导(`AuthPrompter` 未被调用),`load_config` 原样返回 `Err`(配置错误浮现,不被引导静默覆盖)

#### Scenario: 引导取消不留半配置

- **WHEN** 首启零配置,脚本化 `AuthPrompter` 在 provider 选择或 key 输入处取消 / EOF,调用 `load_config_or_onboard`
- **THEN** 返回 `Err(CliError::Auth(AuthError::Cancelled))`,`config.toml` 与 `credentials` 均未生成

### Requirement: headless 首次运行友好报错

系统 SHALL 在 headless 入口 `run_cli`(`--headless`,非交互)检测到首次运行(两份配置文件皆不存在,经 `is_first_run`)时,返回 `CliError::NotConfigured`,MUST NOT 进入交互引导(交互会阻塞管道 / 吞掉 prompt),MUST NOT 触网(在构造 provider 之前返回)。`CliError::NotConfigured` 的 `Display` MUST 为可读单行引导且含命令 `mysteries auth login`(面向发布形态,非 `cargo run -- ...` 调试形态)。该分支 MUST 可离线单测。

#### Scenario: headless 首启返回友好未配置错误(离线)

- **WHEN** user 与 project 两路径皆不存在,以非空 prompt 调用 `run_cli`
- **THEN** 返回 `Err(CliError::NotConfigured)`,构造 provider 之前即返回、全程不触网

#### Scenario: NotConfigured 文案含引导命令

- **WHEN** 对 `CliError::NotConfigured` 取 `Display`(`to_string()`)
- **THEN** 得到可读单行文案,包含子串 `mysteries auth login`,且不含 Debug 风格的枚举包裹

### Requirement: 顶层错误以 Display 呈现

`main` SHALL 以 `Display`(而非 Rust 默认 `Debug`)呈现顶层错误:签名改为 `-> ExitCode`,`Ok(())` → `ExitCode::SUCCESS`;`Err(e)` → `eprintln!("{e}")`(`Display`)后返回 `ExitCode::FAILURE`。使配置 / 装配类错误打可读文案,而非 `Error: Assembly(Config(MissingField("model")))` 这类 Debug 枚举包裹。各 CLI 错误类型(`CliError` / `AssemblyError` / `ConfigError` / `AuthError`)MUST 具备可读的 `Display`(经 `thiserror` `#[error(...)]`),可经各自 `to_string()` 单测。

#### Scenario: 配置缺失错误以可读文案呈现

- **WHEN** 对 `ConfigError::MissingField("model")` 取 `Display`(`to_string()`)
- **THEN** 得到 `missing required config field: model`(可读单行),非 Debug 枚举字面

#### Scenario: 装配错误经 Display 透传内层文案

- **WHEN** 对 `CliError::Assembly(AssemblyError::Config(ConfigError::MissingField("model")))` 取 `Display`
- **THEN** 得到内层可读文案 `missing required config field: model`(`thiserror` `transparent` 透传),不含 `Assembly(Config(...))` 包裹
