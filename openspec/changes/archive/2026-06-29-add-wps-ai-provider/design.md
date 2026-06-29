## Context

`auth login`(`refine-auth-providers` 落定)以交互式单选配置 provider:三预设(OpenAI/Anthropic/DeepSeek,只读 key)+ 自定义(选 kind + 填 base_url/model/key)。WPS 经 ai-kas codeplan 网关提供 OpenAI / Anthropic 两种**协议端点** + 一组**内置模型**,接入只需 base_url(二选一)+ model(目录选一)+ key。现状接 WPS 要走「自定义」手填,繁琐。本 change 加专门的 `WPS AI` 入口。

约束:复用既有装配(`select_provider` 支持 `kind + 自定义 base_url + 逻辑 id 凭据注入`);零新依赖;cli 内核 TDD;不碰 ratatui。

## Goals / Non-Goals

**Goals:**
- `auth login` 候选在 `自定义` 上方加 `WPS AI`。
- `WPS AI` → 方式子选择:OAuth2(占位) / WPS CodingPlan。
- WPS CodingPlan:选协议 → 选内置模型 → 输 key → 写 `provider{id="wps",kind,base_url,model}` + 凭据 `wps`。
- 取消任一步不留半配置;全程离线确定性可测。

**Non-Goals:**
- **不**实装 OAuth2(仅占位 + notice)。
- **不**做 `/model` provider+模型切换器(↑↓ 列已配 provider 及模型并热切换)—— 需多 provider 配置 schema + agent 运行时换 provider + TUI 模态,独立 change。
- **不**新增 `ProviderKind` / **不**改 `select_provider` 或装配层。
- **不**改 `auth list` / `logout` / 三预设 / 自定义路径行为。

## Decisions

- **D1 WPS 复用既有 kind + 自定义 base_url + 逻辑 id,不加新 `ProviderKind`。** WPS codeplan 就是 OpenAI / Anthropic **兼容端点**,落成 `kind=OpenAi|Anthropic` + `base_url=<codeplan 端点>` + `id="wps"` 即可,经既有 `select_provider`(`provider.id` 作凭据名注入)构造。**备选**:加 `ProviderKind::Wps`(弃:要改 `select_provider` / 装配 / `provider-abstraction`,而 WPS 无任何协议差异,纯增负担)。

- **D2 `WPS AI` 不走 `ProviderPreset`,而是独立 `login_wps` + `login_wps_codingplan`。** `ProviderPreset` → 固定单 `ConfigWritePatch`(只读 key);WPS 有**协议**与**模型**两步分支,不是固定 patch。故 `run_auth_login` 选 `WPS AI`(新索引)→ `login_wps`(方式子选择)→ `login_wps_codingplan`(协议→模型→key)。**备选**:塞进 `ProviderPreset`(弃:preset 语义是固定映射,放不下两步选择)。

- **D3 协议 → (kind, base_url) 映射 + 模型从内置目录选;两 base_url 与目录为实现常量。** `OpenAI` → `(OpenAi, WPS_CODEPLAN_OPENAI_BASE_URL)`;`Anthropic` → `(Anthropic, WPS_CODEPLAN_ANTHROPIC_BASE_URL)`。模型经 `select` 从 `WPS_MODELS` 选一。常量值不进 spec 字面(随网关变更只改常量 + 单测),与既有「默认 model 是实现常量」一致。

- **D4 OAuth2 占位 = notice + `Ok` 不写。** 它不是错误也不是用户取消,而是「功能未就绪」,故打 notice、返回 `Ok(())`、不写文件最贴切(用户拍板)。**备选**:返回 `Err`/`Cancelled`(弃:语义不符,且会以非零退出/报错呈现一个正常的占位项)。**边界**:首启 onboarding(`load_config_or_onboard`)里若选 OAuth2 → `Ok` 不写 → 随后 `load_config` 仍 `MissingField`(用户选了明确不支持项的可接受结果);本 change 不为此加特殊提示。

- **D5 菜单索引扩展。** `provider_options = ["OpenAI","Anthropic","DeepSeek","WPS AI","Custom"]`;`run_auth_login` 的 match:`0..=2` → 三预设、`3` → `login_wps`、`_` → 自定义(原 `3` 自定义顺延为末项)。

- **D6 `/model` 切换器明确划出本 change。** 用户确认拆分:WPS auth login 先做;provider+模型 ↑↓ 切换器另开 change(先设计多 provider 配置持久化 + agent 运行时换 provider + TUI 模态)。

## Risks / Trade-offs

- **假设内置模型在两协议端点都可用**(网关页将「端点」与「模型组」分列,未标绑定)→ 按「协议与模型独立选择」实现。若实际某些模型仅限某协议,改 `WPS_MODELS` 为按协议分组的结构 + 调流程即可,不影响其余设计。
- **内置模型目录 / base_url 硬编码** → 网关更新需改常量 + 单测(与既有 preset 默认 model 同策略;可接受,集中一处)。
- **`login_wps_codingplan` 多步注入测试**复用既有 scripted `AuthPrompter`(select / read_secret 脚本)→ 与现有 preset/custom 测试同构,无新测试基建。
