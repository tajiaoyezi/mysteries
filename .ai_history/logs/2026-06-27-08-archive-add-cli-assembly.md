# 2026-06-27 · 08 · archive add-cli-assembly

## 决策

- **全栈装配成可跑 CLI agent**(§12 step3 装配半场,进 TUI 前最后一块 CLI 地基)| 主导:用户拍板拆「config / 装配」后的装配半场 | 依据:§4 / §6 / §7
- **lib/bin 拆分**(D1):`src/lib.rs` 暴露 crate API,`main.rs` 收薄为壳 | 理由:Rust 集成测只能 link lib crate —— `tests/` 的前置
- **装配拆两 seam**(D2):`select_provider`(config→provider)/ `assemble_agent`(provider→Agent)| 这是「kind=mock 怎么落」的答案:production `select_provider` 给 Mock 一条 canned(离线冒烟);**e2e 绕过 switch**、自建多轮 Mock 注入 `assemble_agent` | 弃:Mock 也走固定脚本供 e2e(e2e 需任意脚本)
- **`app.rs`(前端无关)/ `cli.rs`(前端)分模块**(D3)| 锚定紧邻 TUI change「换 decider」,界线现在划好 = TUI 即「加 tui 前端复用 app + 删 cli」干净增删 | 非投机(锚定近期确定需求)
- **`load_config` 缺失容忍 + 路径注入**(D4):存在则读+parse、缺失当空层;路径由调用方注入,home/默认路径解析留 main 薄胶水 → tempdir 离线可测
- **`StdinDecider`:纯 `parse_decision` + 薄 async stdin**(D5):`y`/`yes` 忽略大小写+trim → Allow,余/空行/EOF → Deny(fail-safe);`spawn_blocking` 读 stdin;**权限提示走 stderr**(不污染 StdoutSink 的 stdout 流)| §3 oneshot/两-task 是 TUI 的,CLI 同步即可,不提前造 channel
- **落定前序 provisional 路径**(D6):user `~/.config/mysteries/config.toml`、project `./mysteries.toml`、凭据 `~/.config/mysteries/credentials`;std env 解析(无 home → fail-soft 跳过 user 层)
- **保留 `run_single_turn` 不删**(D7):它是 `conversation` capability 实现、有测试;删它需动 archived spec,超范围
- **typed errors `AssemblyError`/`CliError`,不引 anyhow**(D8);**零新依赖、零新 tokio feature**(D9:no clap、home 用 std env、stdin 走 spawn_blocking)
- **审查修正**:① `run_cli` seed `[System, User]`(主 agent note;实现固定 + `initial_history` 单测);② 权限提示走 stderr(主 agent note);③ wiring 使 `EnvCredentialSource` 变 live → 暴露 `clippy::new_without_default` → 补 `impl Default`(主 agent 审查发现,clippy 首次零警告);④ `agent::DEFAULT_SYSTEM_PROMPT` 改 `pub` 供 cli 复用(DRY,零行为变化)
- **里程碑**:`cargo run -- "<prompt>"` = 可跑 headless CLI agent;装配消解既有 dead_code,**clippy 首次零警告**

## 变更

- 新增 `src/lib.rs` / `src/app.rs` / `src/cli.rs` / `tests/e2e.rs`;`main.rs` 收薄;`agent/mod.rs` `DEFAULT_SYSTEM_PROMPT`→`pub`;`credential/mod.rs` +`impl Default`
- 验证:`cargo test` 95 passed / 1 ignored(94 lib + 1 e2e);`cargo clippy` 零警告;`fmt` 通过;**零新依赖**(`Cargo.toml`/`Cargo.lock` 无 diff)
- archive:`changes/add-cli-assembly` → `changes/archive/2026-06-27-add-cli-assembly`;`specs/` 新增 `cli-runtime`(4 requirements)

## 待决

- **TUI(下个 change,§3 两-task + oneshot 权限 + ratatui)**:新增 tui 前端复用 `app`、换掉 `cli` 的 `StdinDecider`;§3 并发模型届时落地;复用 `设计规范/`
- Anthropic 实装(现选中报 `UnsupportedProvider`)、§5.1 `tool_mode` 降级、内置命令(`/help` 等)、流式/重试收尾(step5)
- `run_cli` 全路径仅冒烟(薄胶水,各组件已离线测);凭据文件权限校验(chmod 600 提示)留后续
- Windows 默认路径用 `%USERPROFILE%\.config\mysteries`(字面 XDG 跨平台,非 `%APPDATA%`),可后续调

## 引用

- change:`add-cli-assembly`(rationale / rejected alternatives 全量见 design.md D1–D9;archive 路径 `changes/archive/2026-06-27-add-cli-assembly`)
- 技术方案 §3 / §4 / §6 / §7 / §9 / §10 / §12 step3
- 前置 change:`add-config-layering`(07)、`add-credential-chain`(05)、`add-openai-live-transport`(06)
- session log:无专属 checkpoint —— 子 agent propose + implement(§5.1 权限路径停点);主 agent review(核 API 对齐、定 stderr/seed note、抓 `EnvCredentialSource` clippy)+ commit / archive
