## MODIFIED Requirements

### Requirement: auth 子命令交互式配置

系统 SHALL 提供 `mysteries auth login` 子命令(由 `main` 分流识别为 `login` / `logout` / `list` 三子命令之一,非 TUI)。**`mysteries auth`(无子命令)MUST 打印帮助、列出 `list` / `login` / `logout` 三子命令并正常退(`Ok`),MUST NOT 默认进入 `login`、MUST NOT 写任何文件**。`auth login` SHALL 以**交互式选择**配置 provider,而非文本输入 provider 名;先经交互式单选让用户从候选(`OpenAI` / `Anthropic` / `DeepSeek` / `WPS AI` / 自定义)选一(`WPS AI` 置于 `自定义` 之上;见「交互式选择(raw mode + 可注入)」)。**三预设(OpenAI / Anthropic / DeepSeek)统一只读 API key**:base_url 用官方默认 endpoint、model 用预设默认(见「provider 预设映射」)。**`WPS AI`** SHALL 引出二级方式子选择(`OAuth2 登录(暂不支持)` / `WPS CodingPlan`),**不**直接走预设 / 自定义路径,详见「WPS AI provider 登录(协议 + 模型选择)」。**自定义** SHALL 选 `kind`(OpenAi/Anthropic,默认高亮 `OpenAi`)+ 输入 base_url(**可空 → 用该 kind 默认端点**)+ 输入 model(非空)+ 输入 key;逻辑 id 取用户逻辑名(空则回落 kind 名)。API key 输入 MUST **隐藏**(不回显;用既有 `crossterm` raw mode 读取、读毕恢复终端态),key MUST 经 `secrecy::SecretString` 承载、MUST NOT 入日志 / 错误 / 提示输出。配置 SHALL 持久化:provider 逻辑 `id` / `kind` / `base_url` / `model` 经 config 写能力 **merge** 入 user `config.toml`(保留其他字段),API key 经 credential 写能力 **upsert** 入 `credentials`(键为该 provider 的**逻辑 id**)。`auth login` 流程 MUST NOT 触网(仅写配置)。**输入读取(provider 选择 + 方式 / 协议 / kind 选择 + 文本 / 模型 / key 输入)MUST 与流程解耦**(可注入),以便离线确定性单测。任一步取消(select 或输入返回取消)/ EOF SHALL 中止且 **不写任何文件**(不留半配置)。

#### Scenario: 无子命令打印帮助(不默认 login、不写文件)

- **WHEN** 运行 `mysteries auth`(无子命令)
- **THEN** 打印帮助并列出 `list` / `login` / `logout` 三子命令,正常退(`Ok`);不进入 login 交互、不写 `config.toml` / `credentials`

#### Scenario: 候选含 WPS AI 且置于自定义之上

- **WHEN** `auth login` 呈现 provider 候选单选
- **THEN** 候选顺序为 `OpenAI` / `Anthropic` / `DeepSeek` / `WPS AI` / `自定义`(`WPS AI` 紧邻 `自定义` 之上)

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

## ADDED Requirements

### Requirement: WPS AI provider 登录(协议 + 模型选择)

系统 SHALL 在 `auth login` 选中 `WPS AI` 后,经交互式单选呈现**两种登录方式**:`OAuth2 登录(暂不支持)` / `WPS CodingPlan`。

- **OAuth2(占位)**:选中 SHALL 打印含「暂不支持、后续考虑支持」语义的 notice、正常退(返回 `Ok`),**MUST NOT 写任何文件**(`config.toml` / `credentials` 不变);本 change 不实装 OAuth2,仅留占位。
- **WPS CodingPlan**:SHALL 依次 ① 交互式选协议(`OpenAI` / `Anthropic`)—— `OpenAI` → `provider.kind = OpenAi`、`base_url` = WPS codeplan **OpenAI** 端点;`Anthropic` → `provider.kind = Anthropic`、`base_url` = WPS codeplan **Anthropic** 端点;② 从**内置 WPS 模型目录**交互式选一 → 写入 `model`;③ **隐藏**读 API key(经 `SecretString`,不回显、不入日志 / 错误 / 提示)。落定 SHALL 经 config 写能力把 `provider{ id = "wps", kind, base_url, model }` **merge** 入 user `config.toml`(保留其他字段),key 经 credential 写能力 **upsert** 入 `credentials`(键 = 逻辑 id `wps`)。

WPS codeplan 两端点 base_url 与内置 WPS 模型目录值为**实现常量**,MUST NOT 在本 spec 钉死字面(随网关 / 模型变更只改常量与单测)。本流程 MUST NOT 触网(仅写配置);所有输入(方式 / 协议 / 模型选择 + key)MUST 经 `AuthPrompter` 可注入,以便离线确定性单测。任一步取消 / EOF SHALL 中止且 **不写任何文件**。WPS 落定的 config 经既有 `select_provider`(`kind=OpenAi/Anthropic` + 自定义 `base_url` + `provider.id="wps"` 作凭据名注入)构造,**不改装配层**。

#### Scenario: WPS AI → OAuth2 占位提示且不写文件(注入,离线)

- **WHEN** 以注入「选 `WPS AI` → 选 `OAuth2`」跑 `auth login`(临时路径)
- **THEN** 打印含「暂不支持」的 notice、返回 `Ok`;`config.toml` 与 `credentials` 均未写;全程不触网

#### Scenario: WPS CodingPlan OpenAI 协议写配置与凭据(注入,离线)

- **WHEN** 以注入「选 `WPS AI` → `WPS CodingPlan` → 协议 `OpenAI` → 选内置模型目录首项 → key=`sk-wps`」跑 `auth login`(临时路径)
- **THEN** `config.toml` 的 `provider.id = "wps"`、`provider.kind = OpenAi`、`base_url` 为 WPS codeplan OpenAI 端点常量、`model` 为所选内置模型;`credentials` 含 `wps = sk-wps`;全程不触网

#### Scenario: WPS CodingPlan Anthropic 协议用 Anthropic 端点

- **WHEN** 以注入「选 `WPS AI` → `WPS CodingPlan` → 协议 `Anthropic` → 选某内置模型 → key=`sk-wps2`」跑 `auth login`
- **THEN** `config.toml` 的 `provider.kind = Anthropic`、`base_url` 为 WPS codeplan Anthropic 端点常量(`provider.id = "wps"`、`model` 为所选项),`credentials` 含 `wps = sk-wps2`

#### Scenario: 选不同内置模型写入对应 model

- **WHEN** WPS CodingPlan 流程中选内置 WPS 模型目录的第 k 项
- **THEN** 写入的 `provider` 段 `model` 等于该项值(取自实现常量目录,不在本 spec 钉死字面)

#### Scenario: WPS CodingPlan 任一步取消不留半配置

- **WHEN** WPS CodingPlan 在协议选择 / 模型选择 / key 输入任一步取消 / EOF
- **THEN** 不写入 `config.toml` 或 `credentials`(既有配置保持原状)
