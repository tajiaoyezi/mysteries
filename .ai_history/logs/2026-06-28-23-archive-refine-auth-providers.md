# 2026-06-28 · 23 · archive refine-auth-providers

## 决策
- 凭据隔离走 **Path B**(逻辑 id 与 kind 正交):DeepSeek = `provider.id="deepseek"` + `kind=OpenAi` + 凭据键 `deepseek`,与 `openai` 完全分离 | 选:Path B(逻辑 id 注入凭据名) | 弃:Path A(DeepSeek 共用 `openai` 凭据槽 → env footgun) | 主导:用户拍 Path B | 依据:design / cli-runtime「注入分离」scenario
- **Q1 反转**:`auth` 无子命令从「默认 login」改为「打印帮助、列 list/login/logout」 | 弃:默认 login(子命令增至三个后遮蔽 list/logout 可发现性) | 主导:用户(参照 opencode auth) | 依据:cli-runtime MODIFY
- **auth list 来源合并**:`collect_credential_sources` 纯函数,file+env 同名合并单条、**env 在前**(反映 `CredentialChain` env 优先解析序);自定义名不参与 env 检测(复用 `EnvCredentialSource` 语义,resolve 必 None) | 主导:用户拍 OQ3 合并 + file_and_env | 依据:design ⑨ / cli-runtime ADD
- **read_secret 真机 bug 根因**:`read_secret_hidden` 漏 `KeyEventKind::Press` 过滤 → select 确认的 Enter Release 穿透(选预设后无法输入 key)+ 输入字符 Press/Release 翻倍 + Ctrl+C(`\x03`)永不命中 | 修:提取 `apply_secret_key` 纯函数(Press 过滤 + modifiers 判 Ctrl+C) | 主导:用户真机暴露 + 主 agent 诊断 | 依据:对称 `read_select` 的 Press 过滤
- **UX 增强**:login/logout 成功提示 + API key 掩码回显(`*`) | 主导:用户真机反馈(无反馈 / 无回显) | 依据:CLI 外壳(不走红绿)

## 变更
- config/mod.rs:`ProviderConfig.id` + `RawProviderConfig.id`(serde default 向后兼容)+ `ConfigWritePatch.provider_id` + resolve 回落 `default_provider_id_for_kind` + merge 字段级 + write 含 id
- provider/openai.rs·anthropic.rs:`credential_name` 字段 + `with_credential_name` 构造 + `complete` 用 `resolve(&credential_name)`;`new`/`default` 回落 kind 名(零回归)
- credential/mod.rs:`remove_credential`(`remove_credential_line` + 原子 0600 + 幂等)、`list_credential_providers`、`EnvCredentialSource` +deepseek、`collect_credential_sources`(`CredentialEntry`/`CredentialOrigin`,file+env 合并)
- cli.rs:`AuthPrompter::select` + `apply_select_key`(↑↓ 环绕纯函数)+ `preset_patch`(三预设常量)+ `login_preset`/`login_custom` + `run_auth_login`/`logout`/`list` + `apply_secret_key`(Press 过滤)+ read_secret 掩码回显 + 成功提示 + `read_select` 终端渲染
- app.rs:`select_provider` 注入 `config.provider.id` 作凭据名;main.rs:auth list/login/logout 分流 + `print_auth_help`(无子命令)
- 主 specs 合并:cli-runtime(+4~2)、config-layering(~3)、credential-source(+1~1)、provider-abstraction(+1)
- 夹带 3 TUI 小修(无独立 change):first_token_at t/s 口径、补全 ↑↓ 优先 scroll、补全去 min(6) 全显
- 验证:cargo build + test 全 target(264 lib + 1 e2e)、clippy --all-targets 零警告、fmt 净、validate 过;真机冒烟全过(无子命令帮助 / login 三预设+custom / 掩码不翻倍 / list 来源标注无明文 / logout)

## 待决
- 承前未动:git 身份 `wanglei30` 临时 + `leafiellune` purge
- `/model` 切 deepseek 模型(tasks 7.4 提及 deepseek-v4-flash)用户未测,auth 链路已验证
- read_secret 掩码暴露 key 长度(密码框惯例,用户接受)
- OQ1 自定义名 `{UPPER}_API_KEY` env 通配(不做)、OQ5 list 并列 active config(不做)留后续
- provider `with_retry_policy` 私有→pub(sub-agent 顺手扩,可选收窄)

## 引用
- change:refine-auth-providers(archive:changes/archive/2026-06-28-refine-auth-providers;4 capability delta)
- 前置:Path A→B 重写(用户拍 B);tui-activity-status(22)
- **流程教训(双向越界)**:
  - **sub-agent 越界** → 跳过红灯停点④⑤⑥,task1→7 一路平推、tasks 零勾、fmt 未收尾、零回报;主 agent 以「事后一次性全量逐文件复核」替代「逐红灯确认」(代码质量过关,但流程实质破坏);后续追加约束 prompt 收束
  - **主 agent 越界** → read_secret bug 修复时主 agent 亲自写代码(应派 sub-agent);用户纠正后,后续 5 处(成功提示 / 掩码 / list propose / list 红绿 9.1 / 9.3-9.7)全回归「派 sub-agent → 独立 review(读落盘 + 跑门 + git status 验越界)」
  - 范围:并入(用户选 merge_into_current)而非新 change;单 commit(tui/mod.rs 含 config id 连带,TUI 小修无法干净分离)
- memory:
  - review-read-impl-not-just-green-tests(强化:read_secret 真机 bug = 测试绿 264 + clippy + 静态审查仍漏 Windows Press/Release 穿透,真机才暴露)
  - agent-boundary-discipline(新:主/子 agent 双向越界;主 agent 职责 = 诊断 / review / git / openspec / 决策记录,写代码派 sub-agent)
  - red-light-runtime-not-compile-error(延续:红灯⑦ stub 返空运行时 RED)
