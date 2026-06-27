## 1. lib/bin 拆分

- [x] 1.1 新建 `src/lib.rs`:声明全部 `pub mod`(`provider` / `agent` / `tool` / `permission` / `credential` / `config` / `error` + 新 `app` / `cli`);按需 re-export 常用类型
- [x] 1.2 `src/main.rs` 收薄为壳(暂调既有路径占位);`cargo build` + `cargo test` 全绿(纯结构搬迁,既有单测随模块走、行为不变 —— 验证无回归)

## 2. app.rs · load_config(TDD · tempdir)

- [x] 2.1 【红】写 `load_config` 测试(tempdir 注入路径):两层存在 → project 覆盖 user;user 缺失 → 容忍、project 单层 resolve;存在但非法 TOML / 缺必填 → `Err`;确认失败
- [x] 2.2 【绿】实现 `load_config(user_path, project_path) -> Result<Config, AssemblyError>`(存在则读+`config::parse`、缺失当 `RawConfig::default`;`merge`+`resolve`)+ `AssemblyError`(`Config`/`Io`/`UnsupportedProvider`,见 design D8)
- [x] 2.3 【重构】清理

## 3. app.rs · select_provider(TDD · 离线三 kind)

- [x] 3.1 【红】写 `select_provider` 测试(构造不触网):`OpenAi` → `Ok`(真实 provider);`Anthropic` → `Err(UnsupportedProvider)`;`Mock` → `Ok`(canned `MockProvider`,可离线调用);确认失败
- [x] 3.2 【绿】实现 `select_provider(&Config, CredentialChain) -> Result<Box<dyn Provider>, AssemblyError>`(`OpenAi`:`base_url` 有则 `OpenAiProvider::new`、无则 `::default`;`Mock`:`MockProvider::new(canned)`;`Anthropic`:err,见 design D2)
- [x] 3.3 【重构】清理

## 4. app.rs · default_registry + assemble_agent(TDD)

- [x] 4.1 【红】写测试:`default_registry()` 含 7 工具(名集合);`assemble_agent(provider, &config, decider)` 得到的 `Agent` 以 `config.model`/`max_iterations` 构造(经一次 Mock 驱动 run 间接断言工具可被调度);确认失败
- [x] 4.2 【绿】实现 `default_registry()`(注册 `ListDirTool`/`ReadFileTool`/`GlobTool`/`GrepTool`/`WriteFileTool`/`EditFileTool`/`RunShellTool`)+ `assemble_agent`(`Agent::new(provider, registry, decider, config.model.clone(), config.max_iterations)`)
- [x] 4.3 【重构】清理

## 5. cli.rs · StdinDecider(强制 TDD · 停点[新权限路径])

- [x] 5.1 【红 · 停点】写 `parse_decision` 测试:`y`/`Y`/`yes`/带空白 → `Allow`;`n`/空/其他/EOF → `Deny`(fail-safe);确认失败。**贴 `StdinDecider`/`parse_decision` 草案 + 失败输出,停下等用户确认**(CLAUDE.md 折中档:新权限路径首次成型)
- [x] 5.2 【绿】实现 `parse_decision(&str) -> PermissionDecision`(纯)+ `StdinDecider`(impl `PermissionDecider`:展示工具名+args、`spawn_blocking`+`std::io::stdin` 读一行、`parse_decision`,EOF→`Deny`,见 design D5);`StdoutSink` 从 main 迁入
- [x] 5.3 【重构】清理

## 6. cli.rs · run_cli + main 薄胶水

- [x] 6.1 实现 `run_cli(paths, prompt) -> Result<(), CliError>`(`load_config` → 建 `CredentialChain`(`Env`+`File(默认凭据路径)`)→ `select_provider` → `assemble_agent`(`StdinDecider`)→ seed `[System, User(prompt)]` → `StdoutSink` 跑 `Agent.run`)+ `CliError`(见 design D8)
- [x] 6.2 `src/main.rs` 薄胶水:解析默认路径(std env `HOME`/`USERPROFILE` 拼 `~/.config/mysteries/{config.toml,credentials}` + `./mysteries.toml`,见 design D6)→ 读 prompt(`env::args`/stdin,无 clap)→ 调 `cli::run_cli`;`cargo run "<prompt>"` 冒烟(main 不走 red-green)

## 7. tests/ 端到端(首个 tests/,hermetic)

- [x] 7.1 写 `tests/e2e.rs`:`use mysteries::app::assemble_agent` 等;自建 `MockProvider` 脚本(轮1 → `write_file` tool_call、轮2 → 终复)+ tempdir cwd + 脚本化 decider(放行)+ `CaptureSink` → `assemble_agent` → `Agent.run` → 断言 tempdir 文件被创建/改动、最终文本、历史正确。**绕过 `select_provider`/`run_cli`,注入 Mock**(见 design D2);全离线 hermetic

## 8. 收尾

- [x] 8.1 `cargo build`、`cargo test` 默认全绿且**不触网**(装配/loader/decider/e2e 全离线)、`cargo fmt`(可选 clippy);真实 OpenAI 仅 `#[ignore]`(若加)
- [x] 8.2 自检:`cli-runtime` spec 的 4 条 ADDED requirements 全有测试落点;落定路径(config 两层 + 凭据文件,design D6)已写入;偏离已标注(D7 保留 `run_single_turn`、D9 零新依赖);`cargo run "<prompt>"` = 可跑 CLI agent
