# cli-runtime Specification

## Purpose
TBD - created by archiving change add-cli-assembly. Update Purpose after archive.
## Requirements
### Requirement: 配置驱动的 provider 选择

系统 SHALL 提供 `select_provider(&Config, CredentialChain) -> Result<Box<dyn Provider>, AssemblyError>`,按 `config.provider.kind` 选择:`OpenAi` → 真实 `OpenAiProvider`(`base_url` 取 `config.provider.base_url`,有则用、无则默认 endpoint;凭据移交 `CredentialChain`);`Anthropic` → 真实 `AnthropicProvider`(`base_url` 取 `config.provider.base_url`,有则用、无则默认 endpoint;凭据移交 `CredentialChain`);`Mock` → `MockProvider`(固定 canned 脚本)。**真实 provider 构造 MUST 把 `config.provider.id`(逻辑 provider 名)作为凭据名注入**(经 provider 的带凭据名构造路径,见 `provider-abstraction`「Provider 凭据名构造注入」),使 provider 据该逻辑 id resolve key,而非固定 kind 默认名;`Mock` 不需凭据名。旧 config(`provider.id` 缺失)经 resolve 已回落 kind 默认名,故注入值等同现状(向后兼容)。选择 / 构造过程 MUST NOT 发起网络请求(凭据缺失等在 run 时经 `ProviderError::Auth` 暴露,非选择期)。

#### Scenario: OpenAi 选中真实 provider(离线构造)

- **WHEN** `config.provider.kind = OpenAi`,调用 `select_provider`
- **THEN** 返回 `Ok(Box<dyn Provider>)`(真实 `OpenAiProvider`),构造期不触网

#### Scenario: Anthropic 选中真实 provider(离线构造)

- **WHEN** `config.provider.kind = Anthropic`,调用 `select_provider`
- **THEN** 返回 `Ok(Box<dyn Provider>)`(真实 `AnthropicProvider`),构造期不触网

#### Scenario: Mock 可离线跑

- **WHEN** `config.provider.kind = Mock`,调用 `select_provider`
- **THEN** 返回 `Ok` 的 `MockProvider`(固定 canned 脚本),无需网络 / 凭据即可被调用

#### Scenario: 注入逻辑 id 作凭据名(分离凭据,离线)

- **WHEN** `config.provider = { id: "deepseek", kind: OpenAi, base_url: Some("https://api.deepseek.com") }`,`CredentialChain` 仅含 `"openai"` 键(不含 `"deepseek"`),`select_provider` 后调用所得 provider 的 `complete`
- **THEN** 返回 `ProviderError::Auth`(provider 按注入凭据名 `"deepseek"` 解析未命中,**未**误用 `"openai"`),全程不触网

#### Scenario: id 缺失回落 kind 名(向后兼容,离线)

- **WHEN** `config.provider.id` 缺失(resolve 回落 `"openai"`)、`kind = OpenAi`,`CredentialChain` 为空,`select_provider` 后调用 `complete`
- **THEN** 返回 `ProviderError::Auth`(按回落名 `"openai"` 解析未命中),行为与本 change 前一致

### Requirement: 两层配置加载(缺失容忍)

系统 SHALL 提供 `load_config(user_path, project_path) -> Result<Config, _>`:对每个路径,存在则读取并 `config::parse`,**不存在则容忍**(当作空层 `RawConfig::default`);再 `config::merge(user, project)` 后 `config::resolve` 为 `Config`。路径由调用方注入(home / 默认路径解析在 main 薄胶水,不在本函数),以便用临时目录离线单测。文件存在但 TOML 非法、或 resolve 缺必填字段 MUST 返回错误(不静默)。

#### Scenario: 两层均存在,project 覆盖 user

- **WHEN** user 与 project 两份 TOML 均存在(project 覆盖部分字段),调用 `load_config`
- **THEN** 得到 merge + resolve 后的 `Config`,project 的字段覆盖 user、未设字段继承

#### Scenario: 缺失文件被容忍

- **WHEN** user 路径不存在、project 路径存在且完整
- **THEN** 不报「文件缺失」错误,以 project 单层(+ 默认)resolve 出 `Config`

#### Scenario: 存在但非法

- **WHEN** 某存在的配置文件 TOML 非法(或 resolve 缺 `model` / `provider.kind`)
- **THEN** 返回错误(不 panic、不静默默认)

### Requirement: stdin y/n 权限 decider

系统 SHALL 提供 `StdinDecider`(impl `PermissionDecider`),用于 CLI(TUI 前过渡):对 `RequiresConfirmation` 工具,展示工具名 + 参数后读取一行,经一个**可单测的纯解析函数**判定 —— `y` / `yes`(忽略大小写与首尾空白)→ `Allow`,其余(含空行 / EOF)→ `Deny`(fail-safe)。stdin 读取 MUST 为薄壳(异步下经 `spawn_blocking`),决策解析与读取解耦。

#### Scenario: 确认输入 y 放行

- **WHEN** 解析用户输入 `"y"`(或 `"Y"` / `"yes"` / 带首尾空白)
- **THEN** 解析为 `PermissionDecision::Allow`

#### Scenario: 非确认输入拒绝(fail-safe)

- **WHEN** 解析用户输入为 `"n"` / 空行 / 其他任意串 / EOF
- **THEN** 解析为 `PermissionDecision::Deny`

### Requirement: 端到端装配与运行

系统 SHALL 提供 `assemble_agent(provider, &Config, decider) -> Agent`:注册全部 **7 个内置工具**、以 `config.model` 与 `config.max_iterations` 构造 `Agent`。CLI 入口 SHALL 在装配后 seed `[System, User(prompt)]` 历史,并经 `StdoutSink` 流式驱动 `Agent.run`。本能力 MUST 可由 `MockProvider` + 临时目录 + 脚本化 decider **端到端离线**验证(hermetic)。

#### Scenario: 装配出的 Agent 持 7 工具并按 config 构造

- **WHEN** 以一个 `Config`(`model` / `max_iterations`)+ 注入的 provider + decider 调用 `assemble_agent`
- **THEN** 得到的 `Agent` 的工具 registry 含 7 个内置工具,且其 `model` / `max_iterations` 取自 `config`

#### Scenario: 端到端多轮任务(hermetic)

- **WHEN** 注入 `MockProvider` 脚本(轮1 → `write_file` 的 tool_call、轮2 → 终复文本),cwd = 临时目录,decider 脚本化放行,跑 `Agent.run`
- **THEN** 临时目录中对应文件被创建 / 改动,最终返回终复文本,工具结果与历史正确(全程离线、不触网)

### Requirement: 配置超时注入 provider

`select_provider` SHALL 用 `config.timeout_secs` 构造所选 provider 的 per-attempt 超时(经 provider 的 timeout-taking 构造器注入 `RetryPolicy` 的 `attempt_timeout = Duration::from_secs(config.timeout_secs)`);重试次数沿用默认常量。OpenAi 与 Anthropic arm 均 MUST 注入该超时(替代此前硬编码 30s)。注入 MUST NOT 触发网络(仍构造期)。

#### Scenario: provider 用配置超时构造

- **WHEN** `config.timeout_secs = 12`,以 OpenAi(或 Anthropic)调用 `select_provider`
- **THEN** 返回的 provider 其 per-attempt 超时为 12s(经其 `RetryPolicy.attempt_timeout` 断言),构造期不触网

### Requirement: auth 子命令交互式配置

系统 SHALL 提供 `mysteries auth login` 子命令(由 `main` 分流识别为 `login` / `logout` / `list` 三子命令之一,非 TUI)。**`mysteries auth`(无子命令)MUST 打印帮助、列出 `list` / `login` / `logout` 三子命令并正常退(`Ok`),MUST NOT 默认进入 `login`、MUST NOT 写任何文件**(推翻本 change 初定的「无子命令默认 login」:子命令增至三个后,默认 login 会遮蔽 `list` / `logout` 的可发现性)。`auth login` SHALL 以**交互式选择**配置 provider,而非文本输入 provider 名;先经交互式单选让用户从候选(`OpenAI` / `Anthropic` / `DeepSeek` / 自定义)选一(见「交互式选择(raw mode + 可注入)」)。**三预设(OpenAI / Anthropic / DeepSeek)统一只读 API key**:base_url 用官方默认 endpoint、model 用预设默认(见「provider 预设映射」)。**自定义** SHALL 选 `kind`(OpenAi/Anthropic,默认高亮 `OpenAi`)+ 输入 base_url(**可空 → 用该 kind 默认端点**)+ 输入 model(非空)+ 输入 key;逻辑 id 取用户逻辑名(空则回落 kind 名)。API key 输入 MUST **隐藏**(不回显;用既有 `crossterm` raw mode 读取、读毕恢复终端态),key MUST 经 `secrecy::SecretString` 承载、MUST NOT 入日志 / 错误 / 提示输出。配置 SHALL 持久化:provider 逻辑 `id` / `kind` / `base_url` / `model` 经 config 写能力 **merge** 入 user `config.toml`(保留其他字段),API key 经 credential 写能力 **upsert** 入 `credentials`(键为该 provider 的**逻辑 id**)。`auth login` 流程 MUST NOT 触网(仅写配置)。**输入读取(provider 选择 + kind 选择 + 文本 / key 输入)MUST 与流程解耦**(可注入),以便离线确定性单测。任一步取消(select 或输入返回取消)/ EOF SHALL 中止且 **不写任何文件**(不留半配置)。

#### Scenario: 无子命令打印帮助(不默认 login、不写文件)

- **WHEN** 运行 `mysteries auth`(无子命令)
- **THEN** 打印帮助并列出 `list` / `login` / `logout` 三子命令,正常退(`Ok`);不进入 login 交互、不写 `config.toml` / `credentials`

#### Scenario: login 预设只输 key 写配置与凭据(注入,离线)

- **WHEN** 以注入输入「选择 `DeepSeek` 预设、key=`sk-ds`」跑 `auth login`,配置 / 凭据指向临时路径
- **THEN** user `config.toml` 的 `provider.id = "deepseek"`、`provider.kind = OpenAi`、`base_url` 为 DeepSeek 预设 base_url、`model` 为 DeepSeek 预设默认 model(其他字段保留),`credentials` 含 `deepseek = sk-ds`(逻辑 id 作键,与 `openai` 分离);全程不触网

#### Scenario: login 自定义输入 kind/base_url/model/key

- **WHEN** 以注入输入「选择 自定义、kind=`Anthropic`、base_url=`https://x.example`、model=`m1`、逻辑名=`myllm`、key=`sk-c`」跑 `auth login`
- **THEN** `config.toml` 的 `provider.id=myllm`、`provider.kind=Anthropic`、`base_url=https://x.example`、`model=m1`,`credentials` 含 `myllm = sk-c`

#### Scenario: 自定义 base_url 可空用默认端点

- **WHEN** 自定义流程 base_url 留空(空行)
- **THEN** 写入的 `provider.base_url` 为 `None`(`select_provider` 用该 kind 默认端点),其余字段照常写入

#### Scenario: login 取消不留半配置

- **WHEN** `auth login` 在 provider 选择或 key 输入处取消 / EOF
- **THEN** 不写入 `config.toml` 或 `credentials`(既有配置保持原状)

#### Scenario: key 隐藏且不入输出

- **WHEN** `auth login` 读取 API key
- **THEN** 输入不回显;key 经 `SecretString` 承载,任何提示 / 错误输出均不含明文 key

### Requirement: auth logout 子命令(移除凭据)

系统 SHALL 提供 `mysteries auth logout` 子命令(由 `main` 分流识别):读取 `credentials` 文件中已配置的 provider 条目(其键为**真实逻辑 id** `openai` / `anthropic` / `deepseek` / 自定义名),经**交互式选择**(↑↓ 环绕 + Enter,Esc/Ctrl+C 取消)选一,再经 `remove_credential` 移除其凭据行(**保留其他 provider 行**)。当无任何已配置凭据(文件缺失或无条目)时,SHALL 以 notice 正常结束(返回 `Ok`,不报错、不进入选择)。取消选择 SHALL **不移除任何凭据**。流程 MUST NOT 触网;选择输入 MUST 可注入,以便离线确定性单测。

#### Scenario: logout 列出真实逻辑名并移除选中、保留其他(注入,离线)

- **WHEN** `credentials` 含 `openai = sk-o`、`deepseek = sk-d` 两逻辑条目,以注入「选择 `deepseek`」跑 `auth logout`(临时路径)
- **THEN** 选择列表含真实逻辑名 `openai` / `deepseek`;移除后 `credentials` 不再含 `deepseek` 行、仍含 `openai = sk-o`;全程不触网

#### Scenario: logout 取消不移除

- **WHEN** `auth logout` 在选择处取消 / EOF
- **THEN** `credentials` 内容不变(不移除任何凭据)

#### Scenario: logout 无已配凭据正常退

- **WHEN** `credentials` 文件不存在或无任何条目时跑 `auth logout`
- **THEN** 以 notice 正常结束(`Ok`),不报错、不 panic

### Requirement: 交互式选择(raw mode + 可注入)

系统 SHALL 为 auth 提供交互式单选能力:经 `AuthPrompter::select(prompt, options) -> Result<Option<usize>, _>` 注入(`Some(idx)` = 选中项,`None` = 取消)。其终端实现 SHALL 用既有 `crossterm` raw mode 渲染候选并高亮当前项,`↑` / `↓` 移动高亮(**首尾环绕**:首项再 `↑` 跳末项、末项再 `↓` 跳首项)、`Enter` 确认、`Esc` / `Ctrl+C` 取消,读毕恢复终端态(**零新依赖**)。**按键归约逻辑 MUST 为可单测纯函数**:给定(当前高亮、候选数、按键)归约为 移动(新高亮)/ 确认 / 取消 / 忽略,不依赖真实终端。

#### Scenario: select 按键归约含首尾环绕(纯函数)

- **WHEN** 对(高亮 = 0、候选 = 3)施加 `↑`,以及对(高亮 = 2、候选 = 3)施加 `↓`
- **THEN** 前者归约为 移动到 2(首项上移环绕到末项),后者归约为 移动到 0(末项下移环绕到首项);`Enter` 归约为确认当前 idx;`Esc` / `Ctrl+C` 归约为取消

#### Scenario: 注入 select 驱动流程(离线)

- **WHEN** 以脚本化 `AuthPrompter`(`select` 返回预置 idx)驱动 `auth login` / `auth logout`
- **THEN** 流程取得该选择并继续,无需真实终端、不触网

### Requirement: provider 预设映射

系统 SHALL 以**可单测纯函数**把所选预设 provider 映射为 `ConfigWritePatch{provider_id, provider_kind, base_url, model}` 与凭据键(= 逻辑 id):`OpenAI` → (id `openai`,`OpenAi`,base_url `None` = 用默认端点,默认 model 常量,键 `openai`);`Anthropic` → (id `anthropic`,`Anthropic`,`None`,默认 model 常量,键 `anthropic`);`DeepSeek` → (id `deepseek`,`OpenAi`,`Some(DeepSeek base_url)`,DeepSeek 默认 model 常量,键 `deepseek` —— OpenAI 兼容端点、与 `openai` 键分离)。默认 model 值为**实现常量**,MUST NOT 在本 spec 钉死字面(随官方更名只改常量与单测)。映射 MUST NOT 触网 / 读文件。

#### Scenario: DeepSeek 预设映射为 patch 与逻辑 id 凭据键(纯函数)

- **WHEN** 对 `DeepSeek` 求预设映射
- **THEN** 得 `provider_id = "deepseek"`、`provider_kind = OpenAi`、`base_url = Some(DeepSeek base_url)`、`model = DeepSeek 默认 model 常量`、凭据键 = `"deepseek"`(与 `"openai"` 分离;不触网 / 不读文件)

#### Scenario: OpenAI / Anthropic 预设用默认端点与各自逻辑 id

- **WHEN** 分别对 `OpenAI` / `Anthropic` 求预设映射
- **THEN** 二者 `base_url` 均为 `None`(交 `select_provider` 用 provider 默认端点),`provider_id` / 凭据键分别为 `"openai"` / `"anthropic"`

### Requirement: auth list 列举凭据来源

系统 SHALL 提供 `mysteries auth list` 子命令(由 `main` 分流识别,非 TUI):列出**当前持有凭据**的 provider 逻辑名及其**来源标注**(`[file]` / `[env]` / `[env, file]`)。来源收集 SHALL 经一个**可注入纯函数**(如 `collect_credential_sources(credentials_path, env_lookup)`)完成:① 从 `credentials` 文件取已配逻辑名(file 来源,沿用既有 file 行解析);② **仅对预设三家**(`openai` / `anthropic` / `deepseek`)经注入的 env lookup 检测其约定变量是否设置(env 来源,**复用 `EnvCredentialSource` 的预设映射语义**);③ 同名合并为**单条**、标注其命中的**全部**来源,标签内 **env 在前、file 在后**(反映 `CredentialChain` env 优先于 file 的解析次序)。**自定义(非预设)逻辑名 MUST NOT 参与 env 检测**(env 变量名为预设约定,自定义名无法预知;与 `EnvCredentialSource` 一致)。当无任何凭据(file 无条目且预设 env 均未设)时 SHALL 打印 notice 正常退(返回 `Ok`,不报错、不 panic)。`auth list` MUST NOT 触网;输出 MUST 仅含 provider 逻辑名与来源标签,**MUST NOT 输出任何 key 明文**。收集逻辑(credentials 路径 + env lookup)MUST 可注入,以便离线、确定性单测。

#### Scenario: 仅 file 来源

- **WHEN** `credentials` 含 `openai = sk-o`、`myllm = sk-m`,注入 env lookup 不含任何 `*_API_KEY`,跑 `auth list`
- **THEN** 列出 `openai [file]` 与 `myllm [file]`(两条);输出不含 `sk-o` / `sk-m` 明文

#### Scenario: 仅 env 来源

- **WHEN** `credentials` 文件缺失或无条目,注入 env lookup 含 `OPENAI_API_KEY`,跑 `auth list`
- **THEN** 列出 `openai [env]`(一条)

#### Scenario: file + env 并存同名合并

- **WHEN** `credentials` 含 `openai = sk-o`,且注入 env lookup 含 `OPENAI_API_KEY`,跑 `auth list`
- **THEN** 列出**单条** `openai [env, file]`(标双来源、env 在前;不重复为两条)

#### Scenario: 自定义名不参与 env 检测

- **WHEN** `credentials` 含 `myllm = sk-m`,即便注入 env lookup 含 `MYLLM_API_KEY`,跑 `auth list`
- **THEN** `myllm` 仅标 `[file]`(自定义名不因 env 命中而标 env)

#### Scenario: 空凭据 notice 正常退

- **WHEN** `credentials` 缺失 / 无条目,且预设三家 env 均未设,跑 `auth list`
- **THEN** 打印 notice、正常退(`Ok`),不报错、不 panic

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

