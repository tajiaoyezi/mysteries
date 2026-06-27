## Context

内核 seam 全部就位且有测试(provider 抽象 / 实传输、Agent Loop、7 工具、权限门、凭据链、配置分层),但 `main` 仍是 bootstrap 单轮 `MockProvider` demo —— 全栈未装配成可跑 agent。本 change 接成 CLI agent(§12 step 3 装配半场,§4 / §6 / §7),并引入首个 `tests/` 端到端(§10)。前序均已 archived,本 change 消费**真实** API:`Config`(pub 字段 + `parse`/`merge`/`resolve`)、`OpenAiProvider::{new(base_url, creds), default(creds)}`、`MockProvider::new(Vec<ModelResponse>)`、`CredentialChain::new(vec![...])`、`Agent::new(provider, registry, decider, model, max_iterations)`、7 个 `pub` 工具结构体、`PermissionDecider`(async)。

约束:Rust 自实现;装配 / 配置 / 权限属 headless 内核(强制 TDD);测试**不依赖真实网络 / FS 状态**(tempdir + Mock + 注入路径);权威次序 code > spec > 推断。本 change 不触及 UI 渲染。

## Goals / Non-Goals

**Goals:**

- lib/bin 拆分,使 `tests/` 可 import crate API。
- 前端无关装配(`load_config` / `select_provider` / `default_registry` / `assemble_agent`)可离线测。
- CLI 前端(`StdinDecider` / `StdoutSink` / `run_cli`)+ 薄 main。
- 落定前序 provisional 默认路径。
- 首个 hermetic 端到端测(Mock + tempdir + 脚本 decider)。

**Non-Goals(留后续):**

- TUI(下个 change,复用 `app`、换掉 `cli` 的 stdin decider)、Anthropic 实装、`tool_mode` 降级、内置命令、流式/重试收尾。
- 删除 `run_single_turn`(保留为 lib API,见 D7)。

## Decisions

- **D1 lib/bin 拆分。** 新建 `src/lib.rs` 声明全部 `pub mod`(provider / agent / tool / permission / credential / config / error + 新 `app` / `cli`);`src/main.rs` 收薄为壳调 `mysteries::cli::run_cli`。**理由**:Rust 集成测(`tests/`)只能 link lib crate、碰不到 bin —— 端到端测的前置即 lib 化。

- **D2 装配拆两个 seam:`select_provider`(config→provider)与 `assemble_agent`(provider→Agent)。** 这是「config `kind=mock` 怎么落」的答案:
  - `select_provider(&Config, CredentialChain) -> Result<Box<dyn Provider>, AssemblyError>`:`OpenAi` → `base_url` 有则 `OpenAiProvider::new(url, creds)`、无则 `::default(creds)`;`Anthropic` → `Err(UnsupportedProvider)`;`Mock` → `MockProvider::new(canned)`(固定 1 条 canned 回复,供 `kind="mock"` 离线冒烟)。
  - `assemble_agent(provider: Box<dyn Provider>, &Config, decider) -> Agent`:`default_registry()`(7 工具)+ `Agent::new(provider, registry, decider, config.model.clone(), config.max_iterations)`。
  - **e2e 绕过 `select_provider`**,自建多轮脚本 `MockProvider` 注入 `assemble_agent`(canned 脚本给不了 e2e 需要的「轮1 tool_call、轮2 终复」)。备选:Mock 也走 switch 的固定脚本供 e2e(弃:e2e 需任意脚本,固定脚本不可行)。

- **D3 模块归属:`app.rs`(前端无关)/ `cli.rs`(CLI 前端)。** `app.rs` = `load_config` / `select_provider` / `default_registry` / `assemble_agent` / `AssemblyError`;`cli.rs` = `StdinDecider` / `parse_decision` / `StdoutSink` / `run_cli` / `CliError`。**理由**:TUI 是已知紧邻的下一个 change 且明确「换掉 stdin decider」—— app(可复用装配)与 cli(前端)的界线现在划好,TUI 即「加 tui 前端复用 app + 删 cli」的干净增删。**非投机**(锚定近期确定需求,非假想扩展)。备选:全塞一个 `app.rs`(弃:TUI 时要拆解 stdin 与装配的纠缠)。

- **D4 `load_config` 缺失容忍、路径注入。** 每路径:存在 → 读 + `parse`;不存在 → `RawConfig::default`(空层);再 `merge` + `resolve`。返回 `Result<Config, AssemblyError>`(`Io` 读失败 / `Config(ConfigError)` 解析或必填缺失)。**路径由调用方注入**(tempdir 离线测);home / 默认路径解析在 main 薄胶水。**理由**:把唯一的真实 FS 读集中在注入路径的薄 loader,单测用 tempdir 即 hermetic。

- **D5 `StdinDecider`:决策纯函数 + 薄读取。** `parse_decision(&str) -> PermissionDecision`(纯:`trim().to_lowercase()` ∈ {`y`,`yes`} → `Allow`,余 → `Deny`),单测覆盖;`decide`(async)用 `tokio::task::spawn_blocking` + `std::io::stdin().read_line` 读一行再 `parse_decision`,EOF / 读失败 → `Deny`(fail-safe,呼应既有 gate「通道断=拒绝」)。preview = 工具名 + 参数(JSON);write/edit 的 diff 预览**最小化**,完整 diff 留 TUI(§8)。**理由**:决策可测、IO 薄;§3 的 oneshot/两-task channel 是 TUI 的,CLI 同步读即可,**不提前造 channel 机制**。

- **D6 落定 provisional 默认路径。**
  - user config:`~/.config/mysteries/config.toml`(`$HOME` / Windows `%USERPROFILE%` + `\.config\mysteries\config.toml`)。
  - project config:`./mysteries.toml`(cwd 相对)。
  - `FileCredentialSource` 默认凭据文件:`~/.config/mysteries/credentials`(与 config 同目录,`provider = key` 行,建议 chmod 600)。
  解析这些用 std env(main 薄胶水)。**理由**:匹配 §7 / 前序所述字面 XDG 路径;凭据与 config 同目录便于用户管理。

- **D7 保留 `run_single_turn`,不删。** main 改走 `run_cli`(多轮)后 `run_single_turn` 成 main-unused,但它是 `conversation` capability 的实现、有测试。删它需动 archived 的 conversation spec(REMOVED + migration),超本 change 范围。**理由**:最小破坏;留作 lib API(其测试维持其存活)。备选:删除(弃:动既有 capability,范围外)。

- **D8 错误类型 dep-free。** `AssemblyError`(`thiserror`:`Config(#[from] ConfigError)` / `Io(String)` / `UnsupportedProvider`)在 `app.rs`;`CliError`(`Assembly(#[from])` / `Agent(#[from] AgentError)` / `Io(String)`)在 `cli.rs`;main 返回 `Result<(), CliError>`。**不引 anyhow**(§9 虽许装配层用 anyhow,但 typed enum 即够且 dep-free,延续既有 typed-error 纪律)。

- **D9 零新增依赖 / 零新 tokio feature。** 不引 clap(只读 prompt,无 flag;沿用 `env::args`)、不引 home crate(std env)、stdin 用 `spawn_blocking`(现有 `rt-multi-thread`,不引 `io-std`)。

## Risks / Trade-offs

- **[lib/bin 拆分牵动现有 `mod` 与 `main`]** → 缓解:纯结构搬迁(模块挪进 lib、`StdoutSink` 迁 cli),既有单测随模块走、行为不变;`cargo test` 全绿即验证无回归。
- **[`run_single_turn` 成 dead_code]** → 缓解:D7 标注保留;其 `conversation` 测试维持存活;非缺陷。
- **[`select_provider` 的 OpenAi 真实构造难离线测]** → 缓解:`OpenAiProvider::new/default` 仅构造不触网,测「返回 Ok / provider 类型」即可;真实调用仍 `#[ignore]` smoke。
- **[stdin 与 prompt 都读 stdin 的交互含糊]** → 缓解:prompt 优先取 `env::args`(有则不读 stdin),y/n 才读 stdin;纯交互混用属 CLI 过渡局限,TUI 解决。
- **[默认路径跨平台]** → 缓解:字面 `~/.config` 经 std env 拼;Windows 用 `%USERPROFILE%\.config`;home 解析在 main 薄胶水,失败则跳过 user 层(project + 默认仍可跑)。

## Migration Plan

结构重构(bin→lib+bin)+ 新增 `app`/`cli`/`tests`;既有模块行为不变、测试随迁。main 由单轮 demo 切到多轮 `run_cli`。回滚 = revert 本 change 提交(lib/bin 可整体还原)。无数据迁移。

## Open Questions

- TUI change 落地时,`cli::run_cli` 与 tui 前端如何共享 `app` 装配(预期:tui 新增 `tui/` 前端复用 `app`,替换 `StdinDecider` 为 oneshot/UI 决策)。
- `ToolContext.max_output_bytes` 的 CLI 默认值(定一个 `const`,后续随配置可调)。
- 凭据文件默认路径是否随后续配置文档化 / 加权限校验(chmod 600 提示)。
