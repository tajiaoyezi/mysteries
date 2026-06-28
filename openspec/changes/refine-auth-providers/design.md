## Context

现状(已对 HEAD=ac4b5c1 代码核实):

- **auth 入口**:`main.rs` 仅识别 `args.first() == "auth"` → `run_auth_interactive` → `run_auth(paths, &mut StdinAuthPrompter)`。**无子命令**。
- **auth 流程**(`cli.rs::run_auth`):`AuthPrompter`(`read_line` / `read_secret`,`None`=取消)逐行问 provider(文本 `openai`/`anthropic`)、base_url、model、key → `write_config(ConfigWritePatch{provider_kind, base_url, model})` + `write_credential(cred_name, key)`。`StdinAuthPrompter::read_secret` = `read_secret_hidden`:crossterm `enable_raw_mode` + `read()` 事件循环隐藏输入,`Esc`/`Ctrl+C`→`None`,读毕 `disable_raw_mode`。已有可注入测试 `ScriptedAuthPrompter`。
- **凭据**:`write_credential`(upsert,原子 temp+rename,Unix `0o600`,`expose_secret` 集中、错误不泄明文);`FileCredentialSource`(`provider = key` 行解析);**无 `remove_credential`**。`EnvCredentialSource` 仅映射 `openai`→`OPENAI_API_KEY`、`anthropic`→`ANTHROPIC_API_KEY`。
- **凭据键现由 kind 决定**:`select_provider`(`app.rs`)把 `CredentialChain` 交给 provider;`OpenAiProvider` 内部固定 `credentials.resolve("openai")`(`openai.rs:100`)、`AnthropicProvider` 固定 `resolve("anthropic")`(`anthropic.rs:117`)。`config.provider` 仅 `{kind, base_url, auth_type}`,**无逻辑 provider id**。provider 的 `Auth`(`resolve(name).ok_or(ProviderError::Auth)?`)在 `complete` 开头、**HTTP 之前** fail-fast(凭据名注入可离线断言)。
- `ProviderKind` = `OpenAi` / `Anthropic` / `Mock`(无 DeepSeek 变体)。

约束(CLAUDE.md):纯 Rust、不扩 dependency;auth 流程 / config schema / 凭据 / 预设映射属 headless 内核 → 强制 TDD;凭据安全(原子、`SecretString`、不泄明文)。

**本 change 采纳 Path B(逻辑 provider id 分离,上游已拍板)**:相比初稿 Path A(沿用 kind resolve-name、DeepSeek 共用 `openai` 槽),Path B 让 OpenAI/DeepSeek **各存各 key、可并存**,logout 显真实逻辑名,代价是触及 `config-layering` + `provider-abstraction` 两个额外 capability(故本设计的 Non-Goals 不再排除「改 config schema / 改 provider resolve」)。

## Goals / Non-Goals

**Goals:**
- `auth login` / `auth logout` / `auth list` 子命令;`mysteries auth`(无子命令)打印帮助(列三子命令,**不默认 login**)。login 交互式选 provider(↑↓ 环绕 + Enter + 取消);三预设只输 key;DeepSeek 一等纳入;自定义可配 kind/base_url/model/key。
- `auth list` 列出持有凭据的 provider + 来源标注(`[file]` / `[env]` / `[env, file]`),复用 `EnvCredentialSource` 预设映射检测 env、file 行解析取 file 名。
- 逻辑 provider id 贯通:config schema 加 id、provider 凭据名由 id 注入、env 加 deepseek → 三家各存各 key、可并存。
- `remove_credential` 安全删行;全流程可注入、离线、不触网、可单测。
- **向后兼容**:旧 config(无 id)回落 kind 名,既有 provider 行为逐字节不变。

**Non-Goals(本期不做):**
- 不碰订阅 / OAuth(API key only;OAuth 仍留 2.0)。
- 不改 TUI `/model` 切 model(已有)。
- 不为自定义逻辑名引入 env 变量约定(自定义仅 file 凭据,见决策⑦)。
- 不给 `ProviderKind` 加 `DeepSeek` 变体(DeepSeek 复用 `OpenAi` kind + 逻辑 id `deepseek`,见决策③)。
- 不引 tokenizer / 其他依赖。

## Decisions

### 决策① auth 子命令拆分 + main 分流(Q1 **改判**:无子命令 → 帮助)

- `main` 识别 `auth login` → `run_auth_login`;`auth logout` → `run_auth_logout`;**`auth list` → `run_auth_list`**(见决策⑨)。
- **`mysteries auth`(无子命令)→ 打印帮助**(列出 `list` / `login` / `logout` 三子命令)并正常退(`Ok`),**不默认 login、不写文件**。
- **改判说明**:本 change 初定 Q1「无子命令默认 login」,现参照 opencode 改判为「无子命令打印帮助」——子命令增至三个(list/login/logout)后,默认 login 会遮蔽 list/logout 的可发现性,帮助列子命令更合直觉。spec / proposal 同步改判;`main.rs` 已实现的「无子命令默认 login」分流将在 implement 阶段改为帮助(tasks 9.5)。

### 决策② 交互式选择组件(raw mode + 可注入 + 纯归约,Q2 已定:首尾环绕)

- `AuthPrompter` 加 `select(&mut self, prompt: &str, options: &[&str]) -> Result<Option<usize>, AuthError>`:`Some(idx)`=选中,`None`=取消(`Esc`/`Ctrl+C`)。
- `StdinAuthPrompter::select`:复用 `read_secret_hidden` 的 raw mode 姿势,渲染候选 + 高亮;`↑`/`↓` 移高亮(**首尾环绕**:首项再 ↑ 跳末项、末项再 ↓ 跳首项)、`Enter` 选、`Esc`/`Ctrl+C` 取消。**零新依赖**。
- **可测**:① 流程级——`ScriptedAuthPrompter::select` 返回脚本化 idx;② 按键级——纯函数 `apply_select_key(highlight, len, key) -> SelectAction`(`Move(idx)`/`Confirm(idx)`/`Cancel`/`Ignore`),单测 ↑↓ 环绕 / Enter / Esc / Ctrl+C(headless,🔴 红灯停点)。
- **取消语义**:select 返回 `None` → 流程 `AuthError::Cancelled` → **不写任何文件**。

### 决策③ Path B:逻辑 provider id 分离(config schema + provider 注入 + env)

逻辑 provider id 与 `kind` **正交**:`id` 是凭据键 / 逻辑身份(`openai`/`anthropic`/`deepseek`/自定义名),`kind` 是 wire 协议族(`OpenAi`/`Anthropic`)。DeepSeek = `id=deepseek` + `kind=OpenAi`,无需给 `ProviderKind` 加变体。

**(a) config schema(`config-layering`)**:
- `RawProviderConfig` 加 `id: Option<String>`(`#[serde(default)]`,旧 toml 无该字段 → `None`,**照常解析、不破既有读取**)。
- `ProviderConfig` 加 `id: String`(resolve 后必有值)。
- `resolve`:`id = raw.provider.id` 缺失时 **回落 kind 默认凭据名**(`OpenAi`→`"openai"`、`Anthropic`→`"anthropic"`、`Mock`→`"mock"`)→ 旧 config 行为同现状。
- `merge_provider`:`id` 随 provider 嵌套字段级 merge(`project.id.or(user.id)`)。
- `ConfigWritePatch` 加 `provider_id: String`;`write_config` 写入 `provider.id`。

**(b) provider 凭据名注入(`provider-abstraction`)**:
- `OpenAiProvider`/`AnthropicProvider` 加「凭据名」字段 + **带凭据名的构造路径**(如 `with_credential_name(...)` 或现有构造器加参数);provider 内 `resolve(&self.credential_name)` 替代固定 `resolve("openai")`。
- **既有默认构造器回落 kind 默认名**(`OpenAi`→`openai`、`Anthropic`→`anthropic`)→ 既有 provider 单测(默认/`new`/`default` 构造)**逐字节不变**;`openai-transport`/`anthropic-transport` 的「凭据缺失→Auth」scenario 仍成立(missing 不论名)。
- **最小侵入**:只加构造路径 + 一个字段,不改 `Provider` trait 签名、不改 `complete` 逻辑(仅 resolve 的名来源)。

**(c) select_provider 注入(`cli-runtime`)**:
- `select_provider` 用 `config.provider.id` 作凭据名,经带凭据名的构造器传入 provider。旧 config(id 回落 kind 名)→ 注入 kind 名 → 同现状。

**(d) env(`credential-source`)**:`EnvCredentialSource` 加 `deepseek`→`DEEPSEEK_API_KEY`。**Path B 根除 Path A 的 env footgun**:DeepSeek 用 `deepseek` 键 + `DEEPSEEK_API_KEY`,与 `openai`/`OPENAI_API_KEY` 完全分离,设了 `OPENAI_API_KEY` 不再短路 DeepSeek。

### 决策④ provider 预设表(Path B 凭据键,Q4/Q5 已定)

model 名为**实现常量**(不在 spec 钉死,随官方更名只改常量 + 测试)。调研结论(2026-06-28,context7/web 官方 docs)+ 上游拍板:

| 逻辑 id | `kind` | `base_url`(写入 config) | 默认 model(常量) | 凭据键 | env |
| --- | --- | --- | --- | --- | --- |
| `openai` | `OpenAi` | `None`(→ 默认 `https://api.openai.com/v1`) | `gpt-5.5` | `openai` | `OPENAI_API_KEY` |
| `anthropic` | `Anthropic` | `None`(→ 默认 `https://api.anthropic.com`) | `claude-opus-4-8`(旗舰) | `anthropic` | `ANTHROPIC_API_KEY` |
| `deepseek` | `OpenAi` | `Some("https://api.deepseek.com")` | `deepseek-v4-pro`(可切 `deepseek-v4-flash`) | `deepseek` | `DEEPSEEK_API_KEY` |
| 自定义 | 选 `OpenAi`/`Anthropic` | `Some(输入)` / 空→`None` | 输入 | 用户逻辑名(或按 kind) | 不映射(仅 file) |

- 预设 → patch 映射为**纯函数**(`preset → (ConfigWritePatch{provider_id, provider_kind, base_url, model}, 凭据键)`),headless TDD(🔴 红灯停点)。
- OpenAI/Anthropic 用 `base_url=None`(让 `select_provider` 走 provider `DEFAULT_BASE_URL`),官方改端点无需重配;DeepSeek 必须显式写 `base_url`。
- model 常量(上游确认):OpenAI `gpt-5.5`、Anthropic `claude-opus-4-8`、DeepSeek `deepseek-v4-pro`。

### 决策⑤ login / logout 流程(可注入,Q6/Q7 已定)

- **`run_auth_login(paths, prompter)`**:
  1. `select("选择 provider", [OpenAI, Anthropic, DeepSeek, 自定义])` → `None` 即 `Cancelled`(不写)。
  2. 预设:`read_secret("API key")` → `None` 即 `Cancelled`;`write_config(preset_patch)`(含逻辑 id)+ `write_credential(逻辑 id, key)`。
  3. 自定义:`select("选择 kind", [OpenAi, Anthropic])`(**默认高亮 `OpenAi`**)→ `read_line(base_url)`(**可空 → `None`,用 kind 默认端点**)→ `read_line(model,非空)` → `read_secret(key)`;逻辑 id 用用户逻辑名(或按 kind);任一 `None`/空 model 即 `Cancelled`;写 config + credential。
  4. **写顺序**:先 config 后 credential(沿用现状);任一失败返回错误。
- **`run_auth_logout(paths, prompter)`**:
  1. 读 `credentials` 已配条目(**真实逻辑名** `openai`/`anthropic`/`deepseek`/自定义名)。
  2. **无任何已配凭据(文件缺失 / 无条目)→ 打印 notice 正常退(`Ok`)**,不报错、不进 select。
  3. `select("选择要登出的 provider", [已配名…])` → `None` 即取消(不删)。
  4. `remove_credential(credentials, 选中名)`。
- 二者均**可注入**(`&mut dyn AuthPrompter`)、临时目录、不触网,确定性单测。

### 决策⑥ remove_credential(`credential-source`,安全敏感)

- `remove_credential(path, provider) -> Result<(), CredentialError>`:read-modify-write 删匹配 `provider = key` 行、**保留其他 provider 行与注释**;复用 `write_credential_file`(原子 temp+rename + Unix `0o600`)。
- **无匹配行 / 文件不存在 → 幂等 `Ok`**(logout 列表来自文件理论必命中;幂等更稳)。明文 MUST NOT 入错误。
- 行解析/保留逻辑抽纯函数(`remove_credential_line(content, provider) -> String`)单测(🔴 红灯停点)。

### 决策⑦ env 自定义名策略(Path B 新细节)

- `EnvCredentialSource` 固定映射**预设三家**:`openai`→`OPENAI_API_KEY`、`anthropic`→`ANTHROPIC_API_KEY`、`deepseek`→`DEEPSEEK_API_KEY`。
- **自定义逻辑名 → 返回 `None`(不走 env)**,自定义 provider 仅经 file 凭据。理由:env 名是约定,自定义名无法预知;自定义本就靠 file;避免「自定义名→env 名」转换的大小写/字符歧义。(Open Questions OQ1:是否要 `{UPPER}_API_KEY` 通配,默认不做。)

### 决策⑧ spec 挂载

- 交互 select + provider 预设**并入 `cli-runtime`**(CLI auth 关注点,避免 capability 膨胀)。
- `cli-runtime`:**MODIFY**「auth 子命令交互式配置」(→ login + **无子命令打印帮助、推翻 Q1 默认 login**;main 分流 login/logout/list)、「配置驱动的 provider 选择」(注入凭据名);**ADD**「auth logout」「交互式选择」「provider 预设映射」「**auth list 列举凭据来源**」。
- `credential-source`:**MODIFY**「环境变量凭据来源 EnvCredentialSource」(+deepseek / 自定义不走 env);**ADD**「凭据移除 remove_credential」。
- `provider-abstraction`:**ADD**「Provider 凭据名构造注入」。**注意**:现有 provider-abstraction spec **无**任何「凭据名 / key 解析」requirement(provider 怎么拿 key 从未被 spec 描述、是实现细节),故此为 **ADDED 新关注点**,而非 MODIFY(无对应既有 requirement 可改)。
- `config-layering`:**MODIFY**「TOML 配置解析」(加 `id`)、「解析为运行配置」(id 回落)、「配置写入」(可写 `provider.id`)。

### 决策⑨ auth list:凭据来源收集(纯函数 + 可注入)

- **`run_auth_list(paths)`**:经纯函数 `collect_credential_sources` 收集后逐行打印 `<逻辑名> [<来源…>]`;空 → notice 正常退 `Ok`。
- **`collect_credential_sources(credentials_path, env_lookup) -> Vec<CredentialEntry>`**(`CredentialEntry { name: String, origins: Vec<CredentialOrigin> }`,`enum CredentialOrigin { Env, File }`):
  1. **file**:沿用既有 file 行解析(同 `list_credential_providers`)取已配逻辑名(**保留文件顺序**),每条 `origins = [File]`。
  2. **env**:**仅对预设三家** `[openai, anthropic, deepseek]` 经注入 `env_lookup` 检测约定变量——**复用 `EnvCredentialSource`**:`EnvCredentialSource::with_lookup(env_lookup)` 后对每个预设名 `resolve(name).is_some()`。自定义名经 `EnvCredentialSource::resolve` 必 `None`,**天然不参与 env 检测**(无需额外判断)。
  3. **合并**:file 条目若其名是命中 env 的预设 → `origins = [Env, File]`(**env 在前**,反映 `CredentialChain` env 优先);命中 env 但不在 file 的预设 → 末尾按 `[openai, anthropic, deepseek]` 序追加 `origins = [Env]`。
- **确定性顺序**(便于单测):file 顺序在前 + env-only 预设按预设序在后。
- **来源标签**:`origins` 以 `", "` 连接渲染为 `[env, file]` / `[env]` / `[file]`。
- **安全**:`collect_credential_sources` 只取**逻辑名**与**来源命中布尔**,从不读取 key 值;打印仅名 + 标签,**无明文**(`list_credential_providers` 本就只返名,env 检测只看 `is_some()`)。
- **可测**:`collect_credential_sources`(临时 credentials 路径 + 注入 env_lookup 闭包)为 headless 纯逻辑 → 强制 TDD(🔴 红灯停点⑦);`run_auth_list` 终端打印手动冒烟。
- **复用而非重抄**:env 名表复用决策⑦的 `EnvCredentialSource` 预设映射(不另写一份),file 解析复用既有行解析。

## Risks / Trade-offs

- **config schema 加字段的序列化兼容** → `#[serde(default)]` 使旧 toml(无 `provider.id`)解析为 `None`;resolve 回落 kind 名;既有 config 读取与 `load_config`/`merge`/`resolve` 测保持绿。写回时含 id(新写或被 auth login 重写的 config)。
- **provider 加凭据名字段触既有构造点** → 只加「带凭据名」构造路径 + 字段,默认构造器回落 kind 名 → 既有 provider 单测 byte-for-byte 不变;`select_provider` 是唯一改用新构造的调用点。
- **login 先写 config 后写 credential,credential 失败留「有 config 无 key」** → 沿用现状;下次 login 覆盖;严格事务化记 Non-Goal。
- **model 名硬编码随官方更名失效** → 常量集中 + 单测锁,不进 spec。
- **自定义 provider env 不可用** → 决策⑦ 接受(自定义靠 file);OQ1 可后补通配。
- **跨平台 raw mode select** → 复用既有 `read_secret_hidden` 姿势(Windows 已跑),按键归约纯函数覆盖逻辑,渲染手动冒烟。

## Open Questions(多数已拍板;剩余交上游确认)

> 已拍板锁入设计:~~Q1(无子命令→login)~~ → **Q1 改判:无子命令→打印帮助**(列 list/login/logout,见决策①)/ Q2(select 首尾环绕)/ Q3(**Path B**)/ Q4(Anthropic 旗舰 `claude-opus-4-8`)/ Q5(model 常量 `gpt-5.5`/`claude-opus-4-8`/`deepseek-v4-pro`)/ Q6(logout 无凭据→notice `Ok`)/ Q7(自定义默认 kind=`OpenAi`、base_url 可空→默认端点)。

- **OQ1(env 自定义名)**:自定义逻辑名是否要 `{UPPER}_API_KEY` env 通配?默认**不做**(仅 file),决策⑦。同理 `auth list` 的 env 检测也**仅覆盖预设三家**,自定义名只标 `[file]`(已锁入「auth list」requirement 与决策⑨,非待决)。
- **OQ2(自定义逻辑 id 来源)**:自定义 provider 的逻辑 id 取「用户额外输入的逻辑名」还是「直接用 kind 名(`openai`/`anthropic`)」?默认:**额外问一个逻辑名**(空则回落 kind 名),使多个自定义端点可各存各 key;若嫌步骤多可改为直接用 kind 名(则多个自定义端点共用 kind 槽)。交上游定。
- **OQ3(env+file 同名显示)**:同名 provider 同时有 file 与 env 凭据时如何显示?**建议合并为单条标 `[env, file]`**(env 在前,反映 `CredentialChain` env 优先),已锁入决策⑨ / scenario;备选:拆两条 `openai [env]` / `openai [file]`(更直白但视觉冗余、与「一个 provider 一条」直觉相悖)。交上游确认是否接受合并默认。
- **OQ4(空列表文案)**:`auth list` 无任何凭据时的 notice 文案待定(建议 `No credentials configured. Run 'mysteries auth login' to add one.`);默认走该建议文案,交上游定稿(纯文案、不影响结构)。
- **OQ5(是否并列 active config provider)**:`auth list` 是否同时显示 user `config.toml` 当前选中的 `provider.id` / `model`(标 active)?默认**仅列凭据来源**(本增量范围),config 当前选中视图留后续;交上游定。
