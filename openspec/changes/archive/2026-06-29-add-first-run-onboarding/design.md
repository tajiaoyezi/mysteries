## Context

首次运行 `cargo run`(无任何配置文件)经 `run_tui` → `load_config` → `resolve` 在缺 `model` 时返回 `ConfigError::MissingField("model")`,层层包成 `CliError::Assembly(...)`,最终由 `main` 的 Rust 默认 Termination 打成 `Error: Assembly(Config(MissingField("model")))`(Debug)。项目已具完整 `auth login` 交互流程(`run_auth_login` + `AuthPrompter` trait,可注入、可 Mock,写 `config.toml` + `credentials`,取消不留半配置),但**未在首启时自动引导**。本 change 把「首启零配置」从「致命 Debug 报错」改为「TUI 自动引导 / headless 友好提示」。

约束:守 `add-config-layering` design D4「`model` / `provider.kind` 无安全默认,缺失即致命、不静默兜底」;复用既有 auth 能力、零新依赖;cli/config 内核走 TDD,ratatui 外壳不新增渲染。

## Goals / Non-Goals

**Goals:**
- TUI 首启**两份配置文件皆不存在**时,进 ratatui 前自动跑 `auth login` 引导,配好后重载并正常进入 TUI。
- headless 首启零配置时返回**可读**的未配置错误 + 非零退出,不弹交互。
- 顶层错误以 `Display`(而非 Debug)呈现,使各类错误输出可读。
- 引导触发 / 跳过 / 取消逻辑离线确定性可测。

**Non-Goals:**
- **不**给 `model` / `provider.kind` 写「静默默认值」(违反 D4)。
- **不**新增 ratatui onboarding 视图 / 组件(复用既有 stdin + raw-mode auth 提示)。
- **不**改 `load_config` / `select_provider` / `assemble_agent` 行为。
- **不**改 `auth login` / `logout` / `list` 子命令行为。

## Decisions

- **D1 触发条件 = `is_first_run(paths)` = user 与 project 两文件皆不存在。** 唯一无歧义的「干净零配置」信号。**备选**:以 `resolve` 返回 `MissingField` 为触发(弃:会把 present-but-broken 误判为首次,引导踩用户半成品配置、且违反 D4 —— 残缺是真 misconfig,要让用户看见,不该被引导静默覆盖)。

- **D2 引导前置于进 ratatui 之前。** `run_tui` 在 `load_config`(行 28)与 `TerminalGuard::new()`(行 62)之间无终端态切换,故把引导塞在函数最前,用普通 stdin / raw-mode 提示(`auth login` 的 `select` / `read_secret` 自管 crossterm raw mode、读毕恢复),与 TUI 持久 raw mode 不冲突。**备选**:进 TUI 后用 ratatui 内置 onboarding 视图(弃:需新 TUI 组件 + 状态机 + `设计规范/` 增量,范围远超「复用 auth login」)。

- **D3 编排放 `cli.rs`(前端层),非 `app.rs`(前端无关装配)。** `load_config_or_onboard(paths, prompter: &mut dyn AuthPrompter) -> Result<Config, CliError>` 复用同模块的 `run_auth_login` + `AuthPrompter`;`app.rs` 保持前端无关(不知 prompter / onboarding)。**备选**:放 `app.rs`(弃:把交互引导概念渗入装配层,破坏既有 app/cli 边界)。

- **D4 headless 不引导,返回 `CliError::NotConfigured`。** headless 非交互,弹 `select` 会阻塞管道 / 吞掉 prompt。`run_cli` 开头 `if is_first_run(&paths) { return Err(CliError::NotConfigured); }`,在构造 provider 之前返回(不触网)。新增变体 `#[error("未配置 provider。请先运行: mysteries auth login")]`。**备选**:headless 也走 `load_config_or_onboard`(弃:管道里无交互终端,语义错误)。

- **D5 `main` 改 `-> ExitCode` + `eprintln!("{e}")`(Display)。** Rust 默认 `-> Result` 对 `Err` 打 `Error: {e:?}`(Debug)= 天书来源。`main` 收薄成 `match real_main().await { Ok => SUCCESS, Err(e) => { eprintln!("{e}"); FAILURE } }`,`real_main()` 装现有 body。所有错误经 `thiserror` 的 `Display` 可读呈现(顺带覆盖残缺配置 → `missing required config field: model`)。**备选**:仅 headless 分支手动 `print` + `std::process::exit`(弃:绕过 Drop、只修一条路径;统一 Display 更干净、改善面更广,且维持 main 薄胶水)。

- **D6 取消 → 非零退出**(用户拍板)。`AuthError::Cancelled` 冒泡 → main Display `auth cancelled` + `ExitCode::FAILURE`,不特殊 case 成 exit 0,保持简单。

- **D7 引导 / 报错文案用 `mysteries auth login`**(用户拍板),面向发布形态;`cargo run -- auth login` 仅临时调试,不进文案。

## Risks / Trade-offs

- **引导走 tokio runtime 上的阻塞 stdin** → 与既有 `auth login`(`main` 同步直调 `run_auth_login_interactive`)一致;发生在启动最前、无并发任务,短暂阻塞 executor 无害。
- **`is_first_run` 用 `Path::exists()` 存在 TOCTOU 窗口** → 单用户本地 CLI、启动瞬间无并发写;可接受,不加锁。
- **`main` 改 `ExitCode` 影响所有错误呈现** → 属改善而非回归;各错误类型已具 `thiserror` `Display`,补 `to_string()` 单测兜底关键文案(`MissingField` / `NotConfigured` / transparent 透传)。
- **引导只写 user 层 `config.toml`** → 与 `auth login` 既有语义一致(写 user 层);project 层留给用户手动覆盖,不变。
