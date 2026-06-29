## MODIFIED Requirements

### Requirement: WPS AI provider 登录(协议 + 模型选择)

系统 SHALL 在 `auth login` 选中 `WPS AI` 后,经交互式单选呈现**两种登录方式**:`OAuth2 登录(暂不支持)` / `WPS CodingPlan`。

- **OAuth2(占位)**:选中 SHALL 打印含「暂不支持、后续考虑支持」语义的 notice、正常退(返回 `Ok`),**MUST NOT 写任何文件**(`config.toml` / `credentials` 不变)。
- **WPS CodingPlan**:SHALL 依次 ① 交互式选协议(`OpenAI` / `Anthropic`)—— `OpenAI` → `provider.kind = OpenAi`、`base_url` = WPS codeplan **OpenAI** 端点;`Anthropic` → `provider.kind = Anthropic`、`base_url` = WPS codeplan **Anthropic** 端点;② **隐藏**读 API key(经 `SecretString`,不回显、不入日志 / 错误 / 提示)。**MUST NOT 在 auth 时交互选模型** —— `model` 写入**默认模型实现常量**(`WPS_DEFAULT_MODEL`);具体模型由用户进 TUI 后经 `/model <name>`(现成)或 `/models`(后续)切换。落定 SHALL 经 config 写能力把 `provider{ id = "wps", kind, base_url, model = 默认常量 }` 写入 user `config.toml`(保留其他字段),key 经 credential 写能力 **upsert** 入 `credentials`(键 = 逻辑 id `wps`)。

WPS codeplan 两端点 base_url 与**默认模型**值为**实现常量**,MUST NOT 在本 spec 钉死字面(随网关 / 模型变更只改常量与单测)。本流程 MUST NOT 触网(仅写配置);所有输入(方式 / 协议 + key)MUST 经 `AuthPrompter` 可注入,以便离线确定性单测。任一步取消 / EOF SHALL 中止且 **不写任何文件**。WPS 落定的 config 经既有 `select_provider`(`kind=OpenAi/Anthropic` + 自定义 `base_url` + `provider.id="wps"` 作凭据名注入)构造,**不改装配层**。

#### Scenario: WPS AI → OAuth2 占位提示且不写文件(注入,离线)

- **WHEN** 以注入「选 `WPS AI` → 选 `OAuth2`」跑 `auth login`(临时路径)
- **THEN** 打印含「暂不支持」的 notice、返回 `Ok`;`config.toml` 与 `credentials` 均未写;全程不触网

#### Scenario: WPS CodingPlan OpenAI 协议写默认模型配置与凭据(注入,离线)

- **WHEN** 以注入「选 `WPS AI` → `WPS CodingPlan` → 协议 `OpenAI` → key=`sk-wps`」跑 `auth login`(临时路径,**不选模型**)
- **THEN** `config.toml` 的 `provider.id = "wps"`、`provider.kind = OpenAi`、`base_url` 为 WPS codeplan OpenAI 端点常量、`model` 为 WPS 默认模型常量(`WPS_DEFAULT_MODEL`);`credentials` 含 `wps = sk-wps`;全程不触网

#### Scenario: WPS CodingPlan Anthropic 协议用 Anthropic 端点

- **WHEN** 以注入「选 `WPS AI` → `WPS CodingPlan` → 协议 `Anthropic` → key=`sk-wps2`」跑 `auth login`
- **THEN** `config.toml` 的 `provider.kind = Anthropic`、`base_url` 为 WPS codeplan Anthropic 端点常量(`provider.id = "wps"`、`model` 为 WPS 默认模型常量),`credentials` 含 `wps = sk-wps2`

#### Scenario: WPS CodingPlan 任一步取消不留半配置

- **WHEN** WPS CodingPlan 在协议选择 / key 输入任一步取消 / EOF
- **THEN** 不写入 `config.toml` 或 `credentials`(既有配置保持原状)
