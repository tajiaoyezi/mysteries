# 2026-06-29 · 25 · archive add-first-run-onboarding

## 决策

- **首启零配置从「致命 Debug 报错」改为「TUI 自动引导 / headless 友好报错」** | 主导:用户(诊断 `Assembly(Config(MissingField("model")))` 后拍板「第一次不该报错、该引导」)| 依据:gh/aws 式 onboarding 惯例 + 既有 `auth login` 能力
- **关键区分:「不报错」≠「静默写默认值」** | 选:引导用户去配 | 弃:给 `model`/`provider.kind` 写默认值(违反 add-config-layering D4 —— 无安全通用默认,静默兜底=「跑起来连错 provider」)
- **D1 触发条件 = `is_first_run` = user 与 project 两文件皆不存在** | 弃:以 `resolve` 返回 `MissingField` 触发(会把 present-but-broken 误判为首次、引导踩半成品配置、违反 D4「残缺=真 misconfig 要让用户看见」)
- **D2 引导前置于进 ratatui 之前** | `run_tui` 在 `load_config`(行 28→29)与 `TerminalGuard`(行 62)间无终端态切换,引导走 `auth login` 既有 stdin/raw-mode 提示、读毕恢复 | 弃:ratatui 内置 onboarding 视图(需新组件+状态机+设计规范增量)
- **D3 编排 `load_config_or_onboard` 放 `cli.rs`(前端层),非 `app.rs`(前端无关装配)** | 复用同模块 `run_auth_login`+`AuthPrompter`;`app.rs` 不知 prompter/onboarding | 弃:放 `app.rs`(破坏 app/cli 边界)
- **D4 headless 不引导,返回 `CliError::NotConfigured`** | 非交互弹 select 会阻塞管道/吞 prompt;在构造 provider 前返回(不触网) | 弃:headless 也走 `load_config_or_onboard`(管道无交互终端)
- **D5 `main` 改 `-> ExitCode` + `eprintln!("{e}")`(Display)** | Rust 默认 `-> Result` 对 Err 打 `Error: {e:?}`(Debug)= 天书来源;`real_main()` 装原 body,`main` 仅 Ok/Err→ExitCode | 弃:仅 headless 分支手动 print+`process::exit`(绕 Drop、只修一条路径)
- **D6 取消引导 → 非零退出**(`AuthError::Cancelled` 冒泡 → Display `auth cancelled` + FAILURE)| 主导:用户拍板 a(不特殊 case 成 exit 0,保持简单)
- **D7 引导/报错文案用 `mysteries auth login`**(发布形态)| 主导:用户拍板 b(`cargo run -- auth login` 仅临时调试,不进文案)
- **审查过程(卡点 A · Rust TDD 红灯冲突)**:新 public 符号被测试引用、未声明 → 只能 E0432;与 CLAUDE.md「红:非编译错」+「只写测试」两子句在 Rust 下冲突 | 裁决:用 `unimplemented!()` 签名桩(非实现、零行为)使编译通过 → 测试运行时以「not implemented」失败,得合规红;弃选项1(直接写实现,collapse 红绿)、选项2(load_config 转发桩,fall-through 部分行为致 test 2/3 红步即绿)
- **审查修正**:① model 断言改用常量(`OPENAI_DEFAULT_MODEL`/`ANTHROPIC_DEFAULT_MODEL`→`pub(crate)`),不钉死字面(随官方更名只改常量);② 删 test 1 未用的 `CliError` import(clippy 零警告);③ `CliError` 加 `PartialEq/Eq` 供 `assert_eq!` —— 内层(`AgentError` 等)本就具备,git 未动 `error.rs`,**无级联**

## 变更

- `src/cli.rs`:+`CliError::NotConfigured`;+`is_first_run` / `load_config_or_onboard`;`run_cli` 首启分支;`OPENAI_DEFAULT_MODEL`/`ANTHROPIC_DEFAULT_MODEL`→`pub(crate)`;+8 测试(§2 ×4 / headless ×1 / Display ×3)+ `PanicAuthPrompter`
- `src/main.rs`:抽 `real_main()`;`main -> ExitCode` + `eprintln!("{e}")`(Display)
- `src/tui/mod.rs`:`run_tui` 在 `TerminalGuard` 前经 `load_config_or_onboard`
- 验证:`cargo test` 272 passed + e2e 1 / 2 ignored;`cargo clippy --all-targets -D warnings` 零警告;`openspec validate --strict` 过;冒烟 —— TUI 首启拉起 provider 选择、headless 首启打友好文案 + exit 1
- archive:`changes/add-first-run-onboarding` → `changes/archive/2026-06-29-add-first-run-onboarding`;`specs/cli-runtime` 新增 3 requirements(首启 TUI 引导 / headless 友好报错 / 顶层错误 Display 呈现)

## 待决

- 取消引导后非零退出(选简单);日后若视「主动放弃」非错误可改 exit 0
- `is_first_run` 用 `Path::exists()` 有 TOCTOU 窗口 —— 单用户本地 CLI、启动瞬间无并发写,可接受不加锁
- 引导仅写 user 层 `config.toml`(与 `auth login` 既有语义一致);project 层留用户手动覆盖

## 引用

- change:`add-first-run-onboarding`(rationale / rejected alternatives 全量见 design.md D1–D7;archive 路径 `changes/archive/2026-06-29-add-first-run-onboarding`)
- 前置 change:`add-cli-assembly`(08,app/cli 边界、`load_config`、`StdinDecider`)、`refine-auth-providers`(23,`auth login`/`AuthPrompter`/preset 映射)、`add-config-layering`(07,D4「缺必填致命、不静默兜底」)
- session 主导:用户发起「首启报错」诊断 → brainstorming(选「TUI 自动引导」+「headless 友好报错」)→ OpenSpec propose → 子 agent implement(卡点 A 红灯停点)→ 主 agent review(独立跑 test/clippy、核 D1–D7、抓字面→常量 / 未用 import)
