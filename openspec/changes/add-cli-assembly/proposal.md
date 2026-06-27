## Why

内核的每个 seam 都已就位且有测试(provider 抽象 / 实传输、Agent Loop、7 工具、权限门、凭据链、配置分层),但 **`main` 仍是 bootstrap 的单轮 `MockProvider` demo** —— 全栈还没被装配成一个真正能跑的 agent。本 change 把这些 seam 接成一个**可跑的 CLI agent**(技术方案 §12 step 3 的装配半场,进 TUI 前最后一块 CLI 地基),并引入**首个 `tests/` 端到端集成测**(§10 端到端引入点)。TUI(下个 change)将复用这里的装配、把 stdin decider 换成 §3 的 oneshot/UI。

## What Changes

- **lib/bin 拆分**:新建 `src/lib.rs` 暴露 crate API(声明现有所有 `pub mod` + 新增 `app` / `cli`),`src/main.rs` 收薄成壳 —— 让 `tests/` 能 `use mysteries::...`(集成测只能碰 lib,碰不到 bin)。
- **`src/app.rs`(前端无关装配,放 lib、可测)**:
  - `load_config(user_path, project_path) -> Result<Config, AssemblyError>`:存在则读 + `config::parse`,缺失容忍(当空层),`config::merge` 后 `config::resolve`;路径由调用方注入(home 解析留 main 薄胶水)。
  - `select_provider(&Config, CredentialChain) -> Result<Box<dyn Provider>, AssemblyError>`:`OpenAi` → 真实 `OpenAiProvider`(`base_url` 取 config,有则 `::new`、无则 `::default`,凭据移交 `CredentialChain`);`Anthropic` → `Err(UnsupportedProvider)`;`Mock` → `MockProvider`(固定 canned 脚本)。构造**不触网**(凭据缺失在 run 时经 `ProviderError::Auth` 暴露)。
  - `default_registry() -> ToolRegistry`(注册全部 7 工具)+ `assemble_agent(provider, &Config, decider) -> Agent`(以 `config.model` + `config.max_iterations` 构造)。
  - `AssemblyError`(`thiserror`:`Config` / `Io` / `UnsupportedProvider`)。
- **`src/cli.rs`(CLI 前端,放 lib)**:`StdinDecider`(impl `PermissionDecider`)+ 纯 `parse_decision(&str)`、`StdoutSink`(从 main 迁入)、`run_cli(paths, prompt) -> Result<(), CliError>`、`CliError`。
- **`src/main.rs` 薄胶水**:解析默认路径(home 经 std env)→ 读 prompt(沿用 `env::args`/stdin)→ `mysteries::cli::run_cli(...)`。
- **落定前序 provisional 路径**:user config `~/.config/mysteries/config.toml`、project `./mysteries.toml`、`FileCredentialSource` 默认凭据文件 `~/.config/mysteries/credentials`(`provider = key` 行,建议 chmod 600)。
- **首个 `tests/e2e.rs`(hermetic)**:`MockProvider` 脚本(轮1 → `write_file` tool_call、轮2 → 终复)+ 真实 7 工具 + tempdir cwd + 脚本化 decider → `assemble_agent` → `Agent.run` → 断言文件改动 + 输出。

### 三个澄清点的定夺(proposal 内定)

1. **config `kind = mock` 怎么落**:**两者分工**。production 的 `select_provider` 对 `Mock` 返回带**固定 canned 脚本**的 `MockProvider`(让 `kind="mock"` 能离线无 key 跑通装配冒烟);**e2e 测试绕过 `select_provider`**,自建多轮脚本 `MockProvider` 注入 `assemble_agent`(e2e 需要 `select_provider` 的 canned 脚本给不了的特定多轮脚本)。关键设计 = 把「config→provider 选择」(`select_provider`)与「provider→Agent 装配」(`assemble_agent`)拆成两个 seam。
2. **装配/loader 放哪 + tests API 面**:前端无关的 `load_config` / `select_provider` / `default_registry` / `assemble_agent` 放 **`src/app.rs`**;CLI 专属的 `StdinDecider` / `StdoutSink` / `run_cli` 放 **`src/cli.rs`**。**tests API 面** = `mysteries::app::assemble_agent`(注入 provider)+ `default_registry`、`provider::mock::MockProvider`、`config::Config`、`agent::message::Message`、`tool::ToolContext`、`permission::{PermissionDecider, PermissionDecision}`。拆 app/cli 两模块(非一个)是**锚定已知紧邻的 TUI change**(明确「换掉 stdin decider」):app=可复用装配、cli=前端,界线现在划好,TUI 即「加 tui 前端复用 app + 删 cli」的干净增删,非投机。
3. **clap / home / stdin**:**不引 clap**(只读 prompt,无 flag/subcommand,沿用 `env::args`);**不引 home crate**(home 解析是 main 薄胶水、不单测,用 std env `HOME`/`USERPROFILE` 拼字面 `~/.config/...`;`dirs::config_dir()` 在 macOS/Windows 给非 `~/.config` 的平台目录,反不符所定字面路径);**stdin 不加 tokio feature**(`spawn_blocking` + `std::io::stdin`,现有 `rt-multi-thread` 即可,不引 `io-std`)。→ **本 change 零新增依赖、零新 tokio feature**。

**明确不含**(留后续 change):

- TUI(§3 两-task + oneshot 权限 + ratatui)—— 下个 change,届时新增 tui 前端复用 `app`、换掉 `cli` 的 stdin decider。
- Anthropic provider 实装(`select_provider` 选中 `Anthropic` 即 `UnsupportedProvider` 错误)、§5.1 `tool_mode` 降级、内置命令(`/help` 等)、流式/重试收尾打磨。
- `run_single_turn`(bootstrap 的单轮 demo)保留为 lib API(其 `conversation` capability 测试不变),main 改走 `run_cli` 多轮后它成为 main-unused;**不删除**(删它要动 archived 的 conversation spec,超本 change 范围)。

本 change 不触及 UI 渲染,故不涉及 `设计规范/` 引用。

## Capabilities

### New Capabilities

- `cli-runtime`: 把全栈装配成可跑 CLI agent —— 配置驱动的 provider 选择、两层配置加载(缺失容忍)、stdin y/n 权限 decider、端到端装配与运行(7 工具 + Agent Loop + StdoutSink 流式)。

### Modified Capabilities

<!-- 无。本 change 仅新增 cli-runtime,不改既有 capability 的任何 requirement(Agent Loop / 权限门 / provider 归一化 / 配置分层行为均不变;run_single_turn 保留不删)。 -->

## Impact

- **新增代码**:`src/lib.rs`、`src/app.rs`、`src/cli.rs`、`tests/e2e.rs`;`src/main.rs` 收薄。激活既有 `OpenAiProvider` / `Config` / 7 工具 / `Agent` 的真实装配(消解一批 dead_code)。
- **新增依赖**:**无**(clap / home crate / `io-std` 均按 justify 不引;`toml` / `reqwest` / `secrecy` 等已在)。
- **构建 / 测试**:`cargo build` 通过;装配 / loader / decider 决策走 TDD,**离线**(tempdir + Mock + 注入路径);`select_provider` 三 kind 离线测(构造不触网);首个 `tests/e2e.rs` hermetic(Mock + tempdir);真实 OpenAI 仅 `#[ignore]`。`cargo test` 默认全绿不触网。
- **里程碑**:本 change 后 `cargo run "<prompt>"` 即一个可跑的 headless CLI agent(多轮 Loop + 工具 + stdin 权限)。
- **下游契约**:`app` 的装配 API 供 TUI change 复用;落定的默认路径供后续配置 / 凭据文档化。
