## 1. 错误类型与首启判定(纯增量)

- [x] 1.1 `cli.rs` 新增 `CliError::NotConfigured` 变体,`#[error("未配置 provider。请先运行: mysteries auth login")]`(Display 即友好文案,见 design D4/D7)
- [x] 1.2 `cli.rs` 新增纯函数 `fn is_first_run(paths: &CliPaths) -> bool`(`!user_config.exists() && !project_config.exists()`,见 design D1)
- 注:纯类型 / 纯判定,无网络 / 无外部状态;正确性由 §2–§3 测试间接钉死

## 2. load_config_or_onboard(TUI 引导编排,TDD —— 新路径,红灯停点)

- [x] 2.1 【红】写 `load_config_or_onboard` 测试(tempdir 注入路径 + 复用既有 scripted mock `AuthPrompter`),覆盖 spec 四场景:① 首启零配置 + 选 OpenAI 预设 + 填 key → `Ok(Config{model=OPENAI 预设默认, kind=OpenAi})` 且 `config.toml`/`credentials` 生成;② 已有合法 config + 「调用即 panic」mock → `Ok` 且 prompter 未被调用;③ 文件存在但残缺 + 「调用即 panic」mock → 跳过引导、`load_config` 原样 `Err`;④ 首启 + mock 取消 → `Err(Auth(Cancelled))` 且不写文件。运行确认**失败(非编译错)**,贴测试 + 红灯输出 **→ 停下等确认**
- [x] 2.2 【绿】实现 `pub fn load_config_or_onboard(paths: &CliPaths, prompter: &mut dyn AuthPrompter) -> Result<Config, CliError>`:`if is_first_run(paths) { run_auth_login(&AuthPaths{user_config, credentials}, prompter)?; }` 再 `load_config(...).map_err(Into::into)`(最小实现,见 design D3)
- [x] 2.3 【重构】测试保持绿前提下清理(如 `AuthPaths` 构造去重)

## 3. headless 首启友好报错(TDD)

- [x] 3.1 【红】写 `run_cli` 首启测试(tempdir 两路径皆不存在 + 非空 prompt)→ 期望 `Err(CliError::NotConfigured)`、在构造 provider 前返回(不触网);运行确认失败
- [x] 3.2 【绿】`run_cli` 开头加 `if is_first_run(&paths) { return Err(CliError::NotConfigured); }`(在 `load_config` 之前,见 design D4)
- [x] 3.3 【重构】清理

## 4. 接线:run_tui 调引导 + main 改 Display/ExitCode(外壳)

- [x] 4.1 `tui/mod.rs`:`run_tui` 把 `let config = load_config(&paths.user_config, &paths.project_config)?;` 换成 `let mut prompter = StdinAuthPrompter; let config = load_config_or_onboard(&paths, &mut prompter)?;`(进 `TerminalGuard` 之前,见 design D2)
- [x] 4.2 `main.rs`:抽出 `async fn real_main() -> Result<(), CliError>`(装现有 main 体);`main` 改 `-> ExitCode`,`match real_main().await { Ok(()) => ExitCode::SUCCESS, Err(e) => { eprintln!("{e}"); ExitCode::FAILURE } }`(见 design D5/D6)
- 注:§4 属 shell wiring(ratatui / 进程入口),无新增渲染,不走 red-green;由 §2/§3 内核测试 + §5 文案测试 + 手动冒烟兜底

## 5. 错误 Display 文案单测(钉关键可读性)

- [x] 5.1 【红】写 Display 单测:`CliError::NotConfigured.to_string()` 含 `mysteries auth login`;`ConfigError::MissingField("model").to_string() == "missing required config field: model"`;`CliError::Assembly(AssemblyError::Config(ConfigError::MissingField("model"))).to_string()` 经 transparent 透传 == 内层文案、不含 `Assembly(Config(` 包裹;运行确认失败(`NotConfigured` 尚未加时)
- [x] 5.2 【绿】§1.1 已提供 `NotConfigured` 文案即应转绿(透传 / `MissingField` 文案为既有行为);若不绿,修正 `#[error(...)]` 文案
- [x] 5.3 【重构】清理

## 6. 全量校验

- [x] 6.1 `cargo build` 通过、`cargo clippy` 零警告
- [x] 6.2 `cargo test` 全绿(新增 §2/§3/§5 测试 + 既有测试不回归)
- [x] 6.3 手动冒烟:临时挪开 `~/.config/mysteries/config.toml`,`cargo run` 走引导写配置进 TUI;`echo "hi" | cargo run -- --headless` 打友好文案 + 非零退出
- [x] 6.4 `openspec validate add-first-run-onboarding --strict` 通过
