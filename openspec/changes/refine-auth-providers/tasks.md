# Tasks — refine-auth-providers (Path B)

> TDD 分界:**强制红-绿** = headless 纯逻辑(config schema 逻辑 id 读写兼容 / provider 凭据名注入解耦 / remove_credential / EnvCredentialSource +deepseek / select 按键归约 / provider 预设映射 / login·logout 可注入流程 / **auth list 凭据来源收集 collect_credential_sources**);**手动** = `main` 分流(含无子命令帮助、`auth list` 打印)+ `StdinAuthPrompter::select` 终端渲染冒烟。
> 🔴 **红灯停点**(测试首次成型、贴**运行时** RED 输出后**停下等确认**再写绿):① config schema 逻辑 id 读写兼容(1.x)② provider 凭据名注入解耦(2.x)③ remove_credential + EnvCredentialSource(3.x)④ select 按键归约(4.x)⑤ provider 预设映射(5.x)⑥ login/logout 可注入流程(6.x)⑦ **auth list 凭据来源收集(9.x)**。
> 红灯规范:用脚手架让测试在**运行时失败**(stub 编译通过但返回空 / 错值 / 回落旧行为),**禁用** `todo!()` / 编译错冒充红灯。

## 0. 实施前确认(开 implement 前)

- [x] 0.1 上游已拍板:Q1 无子命令→login / Q2 select 首尾环绕 / Q3 **Path B** / Q4 Anthropic 旗舰 `claude-opus-4-8` / Q5 model 常量(`gpt-5.5` / `claude-opus-4-8` / `deepseek-v4-pro`)/ Q6 logout 无凭据→notice Ok / Q7 自定义默认 kind=OpenAi + base_url 可空。**剩余确认**:design「Open Questions」OQ1(自定义名是否要 `{UPPER}_API_KEY` env 通配,默认不做)、OQ2(自定义逻辑 id 取「额外问逻辑名」(默认)还是「直接用 kind 名」)。未决以 design 推荐默认推进。
- [x] 0.2 **(增量·待上游确认)Q1 改判:无子命令→打印帮助**(原定 login 推翻,见决策①)。新增 `auth list`(决策⑨ / 第 9 节)。剩余确认:OQ3(env+file 同名显示 → 默认合并标 `[env, file]`)、OQ4(空列表文案)、OQ5(是否并列 active config provider,默认不做)。未决以 design 推荐默认推进。

## 1. config-layering — 逻辑 provider id 读写兼容(强制 TDD · 🔴)

- [x] 1.1 【红】先只写测,运行确认**运行时**失败(`id` 字段未加 / resolve 未回落):`RawProviderConfig` 解析含 `id` → `Some`、旧 toml 无 `id` → `None`(照常解析);`resolve` 时 `provider.id` 缺失 → 回落 kind 默认名(OpenAi→openai / Anthropic→anthropic / Mock→mock)、设置时取所设值;`merge_provider` 的 id 字段级 merge(project 覆盖 user);`write_config(patch{provider_id,..})` 回写含 `[provider] id`、保留其他字段。
- [x] 1.2 🔴 **红灯停点①**:贴出 1.1 测试 + 运行时 RED 输出,**停下等确认**(config schema 改动 + 向后兼容回落),再写绿。
- [x] 1.3 【绿】`RawProviderConfig.id: Option<String>`(`#[serde(default)]`)+ `ProviderConfig.id: String` + `ConfigWritePatch.provider_id` + `resolve` 回落 + `merge_provider` 含 id + `write_config` 写 id。确认既有 config 测(load/merge/resolve/write)保持绿(旧 config 无 id → 回落,不破)。

## 2. provider-abstraction — provider 凭据名注入解耦(强制 TDD · 🔴)

- [x] 2.1 【红】先只写测(stub 仍固定 resolve kind 名 → 运行时失败):注入凭据名 `"deepseek"` 构造 `OpenAiProvider`、chain 仅含 `"openai"` → `complete` 返回 `Auth`(未误用 openai);注入 `"deepseek"` + chain 含 `"deepseek"` → 不因 Auth 失败(HTTP 前命中);默认构造(不注入)+ 空 chain → `Auth`(回落 kind 名,零回归)。
- [x] 2.2 🔴 **红灯停点②**:贴出 2.1 测试 + 运行时 RED 输出,**停下等确认**(provider 凭据名注入 = provider-abstraction 行为变更),再写绿。
- [x] 2.3 【绿】`OpenAiProvider`/`AnthropicProvider` 加「凭据名」字段 + 带凭据名构造路径;`resolve(&self.credential_name)` 替代固定名;默认构造回落 kind 名。确认既有 provider 单测(default/new/timeout 构造、Auth-on-missing)**逐字节绿**。

## 3. credential-source — remove_credential + EnvCredentialSource +deepseek(强制 TDD · 🔴)

- [x] 3.1 【红】先只写测(stub no-op / 未加 deepseek → 运行时失败):`remove_credential(path,"deepseek")` 删 deepseek 行、保留 openai 行 + 注释、`resolve("deepseek")`=None;无匹配 / 文件缺失幂等 `Ok`;写回失败错误不含明文;(Unix)`0600`。`EnvCredentialSource` `resolve("deepseek")` 命中 `DEEPSEEK_API_KEY`;自定义名(如 `myllm`)即便注入含 `MYLLM_API_KEY` 也返 `None`。
- [x] 3.2 🔴 **红灯停点③**:贴出 3.1 测试 + 运行时 RED 输出,**停下等确认**(凭据删除·安全敏感 + env 映射扩展),再写绿。
- [x] 3.3 【绿】`remove_credential`(纯函数 `remove_credential_line` + 复用 `write_credential_file` 原子 + `0o600`,无匹配幂等);`EnvCredentialSource` 加 `deepseek`→`DEEPSEEK_API_KEY`,非预设名返 `None`。

## 4. cli — 交互式 select 按键归约 + `AuthPrompter::select`(强制 TDD · 🔴)

- [x] 4.1 【红】先只写测(stub 归约恒返 Ignore → 运行时失败):纯函数 `apply_select_key(highlight, len, key)`——↓/↑ **首尾环绕**移动、Enter→Confirm(idx)、Esc/Ctrl+C→Cancel、其他→Ignore;`ScriptedAuthPrompter::select` 返回脚本 idx。
- [x] 4.2 🔴 **红灯停点④**:贴出 4.1 测试 + 运行时 RED 输出,**停下等确认**(select 选择逻辑 + 环绕),再写绿。
- [x] 4.3 【绿】`AuthPrompter` 加 `select(prompt, options)->Option<usize>`;`apply_select_key` 纯归约(环绕);`ScriptedAuthPrompter::select`。

## 5. cli — provider 预设映射(含逻辑 id)(强制 TDD · 🔴)

- [x] 5.1 【红】先只写测(stub 返错值 → 运行时失败):`preset_patch(provider)` → `(ConfigWritePatch{provider_id,kind,base_url,model}, cred_key)`:OpenAI→(openai,OpenAi,None,常量,openai);Anthropic→(anthropic,Anthropic,None,常量,anthropic);DeepSeek→(deepseek,OpenAi,Some(deepseek base_url),常量,**deepseek**)。model 用集中常量(Q5)。
- [x] 5.2 🔴 **红灯停点⑤**:贴出 5.1 测试 + 运行时 RED 输出,**停下等确认**(预设映射 + 逻辑 id 凭据键),再写绿。
- [x] 5.3 【绿】预设枚举 + 默认 model 常量(集中,便于官方更名)+ `preset_patch` 纯函数(产出 provider_id + 凭据键)。

## 6. cli — login / logout 可注入流程(强制 TDD · 🔴)

- [x] 6.1 【红】先只写测(stub 流程不写文件 → 运行时失败):`run_auth_login`(注入 select+key)——预设(DeepSeek)写 config(id/kind/base_url/model)+credential(**deepseek** 键);自定义(选 kind+base_url 可空+model+逻辑名+key)写对应;取消/EOF 不写任何文件。`run_auth_logout`(注入 select)——列真实逻辑名、移除选中、保留其他;取消不移除;无凭据正常退(notice、Ok)。临时目录、不触网。
- [x] 6.2 🔴 **红灯停点⑥**:贴出 6.1 测试 + 运行时 RED 输出,**停下等确认**(login/logout 流程),再写绿。
- [x] 6.3 【绿】`run_auth_login` / `run_auth_logout`(`&mut dyn AuthPrompter`,可注入);先 config 后 credential;沿用取消不写;logout 读 credentials 逻辑条目 → select → `remove_credential`;无凭据 notice 退。

## 7. main 分流 + 终端 select + select_provider 接线(手动 + 既有测)

- [x] 7.1 `main.rs`:`auth login` / `auth logout` 分流;`auth` 无子命令默认 login(Q1);`run_auth_login_interactive` / `run_auth_logout_interactive` 用 `StdinAuthPrompter`。**(注:「无子命令默认 login」被本增量 9.5 改判为打印帮助;login/logout 分流保留。)**
- [x] 7.2 `StdinAuthPrompter::select`:crossterm raw mode 渲染候选 + 高亮 + ↑↓ 环绕/Enter/Esc/Ctrl+C(复用 `read_secret_hidden` 姿势,读毕恢复);调用纯归约 `apply_select_key`。
- [x] 7.3 `select_provider`:把 `config.provider.id` 作凭据名注入 provider 构造(用 2.3 的带凭据名构造路径);更新既有 `select_provider` 测(注入名 + 回落)+ cli-runtime 新 scenario(注入分离 / 回落)。
- [x] 7.4 手动冒烟(非自动):`mysteries auth login` 选 DeepSeek 输 key → config(id=deepseek)/credentials(deepseek 键)正确;OpenAI + DeepSeek **各存各 key 并存**;`auth logout` 列真实逻辑名、选中移除;Esc 取消不写;进 TUI `/model` 可切 deepseek-v4-flash。

## 8. 收尾验证

- [x] 8.1 `cargo build` 通过;`cargo test` 全绿(新红-绿;既有 `run_auth` 文本流程测 + config/provider 测按 MODIFY 迁移)。
- [x] 8.2 `openspec validate refine-auth-providers --strict` 通过。
- [x] 8.3 确认无新依赖(`cargo tree`);凭据文件 `0600`、无明文入日志;旧 `config.toml`(无 id)仍能 `load_config` + 跑(向后兼容冒烟)。

## 9. 增量:auth list + 无子命令帮助(强制 TDD · 🔴 + 手动)

> **范围增量**:在已实现的 login/logout 之上加 `auth list` 与「无子命令→帮助」(推翻 Q1 默认 login)。下列为 implement 阶段任务,本轮 propose 仅登记、**均未做**;开 implement 前先过 0.2 的剩余确认。

- [x] 9.1 【红】先只写测(stub 返回空 / 错值 → **运行时**失败):纯函数 `collect_credential_sources(credentials_path, env_lookup)` —— 仅 file(逻辑名 + `[File]`、保留文件顺序);仅 env(预设命中 + `[Env]`);file+env 同名**合并单条** `[Env, File]`(env 在前);自定义名即便注入含 `MYLLM_API_KEY` 也只 `[File]`;空(file 无条目 + 预设 env 未设 → 空 `Vec`);env-only 预设按 `[openai, anthropic, deepseek]` 序追加。
- [x] 9.2 🔴 **红灯停点⑦**:贴出 9.1 测试 + 运行时 RED 输出,**停下等确认**(凭据来源收集 + 同名合并 / env 在前 / 确定性顺序语义),再写绿。
- [x] 9.3 【绿】`collect_credential_sources` + `CredentialEntry { name, origins }` / `enum CredentialOrigin { Env, File }`;env 检测**复用** `EnvCredentialSource::with_lookup`(仅预设三家),file **复用**既有行解析;来源标签 `[env, file]` 渲染。确认既有 credential 测保持绿。
- [x] 9.4 `run_auth_list(paths)`:经 `collect_credential_sources`(真实 env lookup)逐行打印 `<名> [<来源…>]`;空 → notice 正常退 `Ok`(文案见 OQ4);输出仅名 + 标签、**无明文**。可注入入口便于离线测(打印 sink 解耦)。
- [x] 9.5 `main.rs` 分流改判:`auth list` → `run_auth_list_interactive`;**`auth` 无子命令 → 打印帮助(列 `list` / `login` / `logout`)正常退**(改 7.1 已实现的「无子命令默认 login」);`login` / `logout` 分流保持不变。
- [x] 9.6 手动冒烟:`mysteries auth`(无子命令)打印帮助;`auth list` 在「只 file / 只 env(设 `OPENAI_API_KEY`)/ file+env 同名」三态下标注正确、**无明文 key**;`auth login` / `auth logout` 回归正常。
- [x] 9.7 收尾:`cargo build` + `cargo test` 全绿(新增 collect 红-绿);`openspec validate refine-auth-providers --strict` 通过;确认零新依赖。
