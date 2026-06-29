## Why

首次运行 `cargo run`(无任何配置文件)时,程序以 `Error: Assembly(Config(MissingField("model")))`(Rust 默认 Debug 原样输出)致命退出 —— 对新用户是天书,且不引导去配置。应像 `gh auth login` / `aws configure` 那样:TUI 首启零配置时自动引导完成 provider 配置;headless 非交互场景给出可读引导文案而非 Debug dump。

## What Changes

- **TUI 首启自动引导**:默认入口(`cargo run`,非 `--headless`)检测到 user/project 两份配置文件**皆不存在**时,在进入 ratatui **之前**自动运行既有 `auth login` 交互流程(选 provider → 填 key → 写 config + credential),配好后**重载配置**并正常进入 TUI。
- **headless 友好报错**:`--headless`(非交互)首启零配置时返回新增 `CliError::NotConfigured`,Display 出友好引导(`未配置 provider。请先运行: mysteries auth login`)+ 非零退出;**不弹交互**(交互会卡住管道 / 吞掉 prompt)。
- **触发条件严格限定**为「user 与 project 两份配置文件皆不存在」(干净零配置 = 首次运行)。文件存在但**残缺 / 非法**仍照旧报错,不引导、不覆盖 —— 守住既有「不静默兜底」决策(present-but-broken 是真 misconfig,要让用户看见)。
- **错误以 Display 呈现**:`main` 从 Rust 默认 `-> Result<(), CliError>`(出错打 `Error: {e:?}`)改为 `-> ExitCode` + `eprintln!("{e}")`,使**所有**错误输出可读(顺带把残缺配置打成 `missing required config field: model` 而非 Debug 枚举)。
- **取消处理**:引导中途取消(`AuthError::Cancelled`)→ Display `auth cancelled` + 非零退出,**不写半份配置**(既有 `run_auth_login` 在写文件前取消,已有测试钉住)。

## Capabilities

### New Capabilities

(无)—— 复用既有 `auth login` / `AuthPrompter` / `write_config` 能力,不引入新 capability。

### Modified Capabilities

- `cli-runtime`: 新增三项 requirement —— ①「首次运行 TUI 自动引导」②「headless 首次运行友好报错」③「错误以 Display 呈现」。既有 `load_config` / `select_provider` / `assemble_agent` / `auth 子命令` 等 requirement **不变**。

## Impact

- **代码**:
  - `src/cli.rs`:新增 `load_config_or_onboard(paths, prompter) -> Result<Config, CliError>` + `is_first_run(paths)` + `CliError::NotConfigured` 变体;`run_cli`(headless)首启分支返回 `NotConfigured`。
  - `src/tui/mod.rs`:`run_tui` 把 `load_config(...)?` 换成 `load_config_or_onboard(&paths, &mut StdinAuthPrompter)?`。
  - `src/main.rs`:`async fn main() -> ExitCode`,`Err` 经 `eprintln!("{e}")`(Display)呈现。
- **复用 / 依赖**:复用既有 `run_auth_login` / `AuthPrompter` / `write_config` / `write_credential`,**零新依赖**。
- **不变**:`load_config` / `select_provider` / `assemble_agent` 行为不动;全程不触网。
- **UI**:不新增 / 不修改 ratatui TUI 组件,引导复用既有 auth 交互提示(stdin + raw mode,已由「交互式选择」requirement 覆盖),故无 `设计规范/` 增量。
- **测试**:cli/config 内核走 TDD(mock `AuthPrompter` + tempdir,离线确定性);ratatui 渲染无新增(无新 insta 快照)。
