## Why

当前 `mysteries auth` 是**纯文本输入**流程(`Provider [openai/anthropic]:` + base_url + model + key,逐行敲),体验差且没有 DeepSeek 预设、没有 logout。参考 opencode:把 auth 拆成 `auth login` / `auth logout` 子命令,login 用**交互式选 provider**(↑↓ 高亮 + Enter),预设 provider 只问 API key(其余用官方默认 base_url + 预设 model),DeepSeek 作为 OpenAI 兼容预设一等纳入,logout 交互式选已配 provider 并安全移除其凭据。降低「配一个 provider」的心智与操作成本。

## What Changes

- **auth 拆子命令**:`mysteries auth login` / `mysteries auth logout` / `mysteries auth list`;**`mysteries auth`(无子命令)打印帮助、列出三子命令(改判:不再默认 login —— 子命令增至三个后默认 login 会遮蔽 list/logout)**。`main` 分流相应调整。
- **login = 交互式选 provider**:crossterm raw mode 渲染候选(**OpenAI / Anthropic / DeepSeek / 自定义**)、↑↓ 移高亮(**首尾环绕**)、Enter 选、Esc/Ctrl+C 取消(取消不写任何文件)。
  - **三预设(OpenAI / Anthropic / DeepSeek)统一只输 API key**(隐藏输入):base_url 用官方默认、model 用预设默认 → 写 `config(逻辑 id + kind + base_url + model)` + `credential(逻辑 id 作键, key)`。三家凭据键分别为 `openai` / `anthropic` / `deepseek`(**各存各 key、可并存**)。
  - **DeepSeek = OpenAI 兼容**:逻辑 id `deepseek`、`kind=OpenAi`、`base_url=https://api.deepseek.com`、默认 model `deepseek-v4-pro`(进 TUI `/model` 可切 `deepseek-v4-flash`);旧 `deepseek-chat`/`deepseek-reasoner` 2026-07-24 弃用,不用。凭据键 `deepseek`(对应 env `DEEPSEEK_API_KEY`),与 OpenAI 的 `openai` 键**分离**。
  - **自定义**:选 `kind`(默认高亮 `OpenAi`)+ 输 base_url(**可空** → 用该 kind 默认端点)+ 输 model + 输 key;逻辑 id 用用户给的逻辑名(或按 kind)。
- **logout = 交互式选已配 provider → 移除其凭据**:读 `credentials` 文件已配条目(**真实逻辑名** `openai`/`anthropic`/`deepseek`/自定义名)→ 交互式选 → 新增 `remove_credential`(read-modify-write 删该行、**保留其他 provider 行 + 注释**,原子写、不泄明文)。无任何已配凭据时打印 notice 正常退(`Ok`,不报错)。
- **list = 列出持有凭据的 provider + 来源标注**:`mysteries auth list` 经纯函数 `collect_credential_sources(credentials_path, env_lookup)` 收集——file 行解析取逻辑名(`[file]`)、对预设三家(`openai`/`anthropic`/`deepseek`)检测约定 env(`[env]`,**复用 `EnvCredentialSource` 映射**)、同名合并标 `[env, file]`(env 在前,反映 `CredentialChain` 优先级);自定义名不参与 env 检测;无凭据打印 notice 正常退。仅输出名 + 来源标签,**不输出 key 明文**。
- **Path B 范围升级(逻辑 provider id 分离)**:
  - **config schema 加逻辑 provider id**:`RawProviderConfig` / `ProviderConfig` 加 `id`(逻辑名,与 `kind`/`base_url` 并存);`ConfigWritePatch` 带 id。旧 config 无该字段 → 回落 kind 名(向后兼容,不破既有读取)。
  - **provider 凭据名解耦**:`select_provider` 把「凭据名」(= 逻辑 id)注入 provider 构造;`OpenAiProvider`/`AnthropicProvider` 不再固定 `resolve("openai")`/`("anthropic")`,改用注入名(未注入则回落 kind 默认名,保既有行为)。
  - **env 凭据来源加 deepseek**:`EnvCredentialSource` 增 `deepseek`→`DEEPSEEK_API_KEY`;自定义逻辑名不走 env(仅 file 凭据)。
- **可注入 / 离线可测**:沿用现 `AuthPrompter` 注入模式,新增 `select`(候选选择)方法;login/logout 流程把「provider 选择 + key 输入」与终端解耦,临时目录写 config/credential、**全程不触网**。
- **进 TUI `/model` 切 model**:已有、不变。

## Capabilities

### New Capabilities
<!-- 无新增 capability:交互式 select 与 provider 预设并入 cli-runtime(见 design.md「决策⑧ 挂载」)。 -->

### Modified Capabilities
- `cli-runtime`:**MODIFIED**「auth 子命令交互式配置」(文本输入 → `auth login`:交互式选 provider + 预设只输 key / 自定义;`main` 分流 login/logout/list、**无子命令打印帮助(改判,不再默认 login)**)、「配置驱动的 provider 选择」(`select_provider` 把逻辑 id 作凭据名注入 provider 构造);**ADDED**「auth logout 子命令(移除凭据)」、「交互式选择(raw mode + 可注入)」、「provider 预设映射(预设 → config patch + 逻辑 id)」、「**auth list 列举凭据来源(file/env 来源标注、同名合并)**」。
- `credential-source`:**MODIFIED**「环境变量凭据来源 EnvCredentialSource」(加 `deepseek`→`DEEPSEEK_API_KEY`;自定义名不走 env);**ADDED**「凭据移除 remove_credential」(read-modify-write 删指定 provider 行、保留其他行与注释,原子、不泄明文)。
- `provider-abstraction`:**ADDED**「Provider 凭据名构造注入」(provider 用构造时注入的凭据名 resolve,而非固定 kind 名;未注入则回落 kind 默认名 → 既有 provider 行为逐字节不变)。
- `config-layering`:**MODIFIED**「TOML 配置解析」(`provider` 嵌套表加 `id`,缺失 → `None`,旧 config 照常解析)、「解析为运行配置(默认与必填校验)」(`provider.id` 缺失 → 回落 kind 默认凭据名)、「配置写入(merge 持久化)」(可写字段含 `provider.id`)。

## Impact

- **code**(本轮 propose 不改,仅登记 implement 触及面):
  - `src/main.rs`:`auth` 分流识别 `login` / `logout` / `list` 子命令;**无子命令打印帮助(列三子命令,改判:不再默认 login)**。
  - `src/cli.rs`:`AuthPrompter` 加 `select(prompt, options) -> Option<usize>`;新增 `run_auth_login` / `run_auth_logout`(可注入)+ provider 预设表/映射(含逻辑 id)+ 自定义分支;`StdinAuthPrompter::select` 用 crossterm raw mode(复用 `read_secret_hidden` 姿势,零新依赖);按键归约抽纯函数(可单测)。新增 `run_auth_list` + 纯函数 `collect_credential_sources`(file 名 + 预设 env 检测、同名合并标来源)。
  - `src/config/mod.rs`:`RawProviderConfig`/`ProviderConfig` 加 `id`;`ConfigWritePatch` 加 `provider_id`;`write_config`/`merge_provider`/`resolve` 处理 id(resolve 缺失→回落 kind 名);serde `#[serde(default)]` 保旧 config 解析。
  - `src/app.rs`:`select_provider` 把 `config.provider.id` 作凭据名注入 provider 构造。
  - `src/provider/{openai,anthropic}.rs`:加「凭据名」字段 + 带凭据名的构造路径;`resolve(&self.credential_name)` 替代固定名;默认构造回落 kind 名(保既有单测)。
  - `src/credential/mod.rs`:`remove_credential(path, provider)`(复用 `write_credential_file` 原子 temp+rename + 0o600;无匹配/缺失幂等 `Ok`);`EnvCredentialSource` 加 `deepseek`→`DEEPSEEK_API_KEY`。
- **OpenAI/Anthropic/DeepSeek 默认 model 名**(`gpt-5.5` / `claude-opus-4-8` / `deepseek-v4-pro`)为**实现常量**,不在 spec 钉死(随官方更名只改常量/测试)。
- **向后兼容**:旧 `config.toml`(无 `provider.id`)→ 解析 None → resolve 回落 kind 名 → `select_provider` 用 kind 名作凭据名 → 行为同现状;既有 provider 单测(默认构造)逐字节不变。
- **deps**:零新增(crossterm 已在)。
- **不受影响**:TUI、agent-loop、headless `run_cli` 主路径不变(仅 `select_provider` 注入凭据名)。
