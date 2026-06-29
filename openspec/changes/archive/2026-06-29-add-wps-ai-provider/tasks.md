## 1. WPS 常量(纯增量)

- [x] 1.1 `cli.rs` 新增 `WPS_CODEPLAN_OPENAI_BASE_URL = "https://ai-kas.kso.net/codeplan/v1"`、`WPS_CODEPLAN_ANTHROPIC_BASE_URL = "https://ai-kas.kso.net/codeplan/anthropic"`、`WPS_MODELS: &[&str]`(8 个:moonshot/kimi-k2.5、deepseek/deepseek-v4-pro、xiaomi/mimo-v2.5-pro、ali/qwen3.7-max、deepseek/deepseek-v4-flash、google/gemini-3.5-flash、zhipu/glm-5、zhipu/glm-5.2;见 design D3)
- 注:纯常量,无网络 / 无外部状态;正确性由 §2 / §3 测试间接钉死

## 2. login_wps_codingplan(协议→模型→key,TDD —— 新路径,红灯停点)

- [x] 2.1 【红】写 `login_wps_codingplan` 测试(tempdir + 复用 scripted mock `AuthPrompter`),覆盖 spec 场景:① 协议 `OpenAI` + 选模型(目录首项)+ key → `(ConfigWritePatch{id="wps", kind=OpenAi, base_url=OpenAI 端点常量, model=首项}, "wps", key)`;② 协议 `Anthropic` → `kind=Anthropic`、`base_url=Anthropic 端点常量`;③ 选第 k 个模型 → `model = WPS_MODELS[k]`;④ 协议 / 模型 / key 任一步取消 → `Err(Cancelled)`。先加 `unimplemented!()` 签名桩使**编译通过**、测试运行时失败(非编译错,见 add-first-run-onboarding 卡点 A 裁决),贴测试 + 红灯输出 **→ 停下等确认**
- [x] 2.2 【绿】实现 `fn login_wps_codingplan(prompter: &mut dyn AuthPrompter) -> Result<(ConfigWritePatch, String, SecretString), AuthError>`:`select` 协议 → `(kind, base_url)`;`select` 模型(`WPS_MODELS`)→ `model`;`read_secret` → key;组 `ConfigWritePatch{ provider_id:"wps", provider_kind:kind, base_url:Some(...), model }`,返回 `(patch, "wps".into(), key)`(见 design D2/D3)
- [x] 2.3 【重构】清理

## 3. login_wps + 菜单接入(TDD)

- [x] 3.1 【红】写测试:① `run_auth_login` 注入「选 `WPS AI`(idx 3)→ `OAuth2`(idx 0)」→ `Ok` 且 `config.toml` / `credentials` **均未写**(占位);② 注入「`WPS AI` → `WPS CodingPlan`(idx 1)→ 协议 OpenAI → 选模型 → key=`sk-wps`」→ `config.toml` 的 `provider.id="wps"`/`kind=OpenAi`/`base_url`=OpenAI 端点/`model`=所选,`credentials` 含 `wps=sk-wps`;③ 候选顺序 `WPS AI` 在 `Custom` 上方。运行确认失败
- [x] 3.2 【绿】`provider_options = ["OpenAI","Anthropic","DeepSeek","WPS AI","Custom"]`;`run_auth_login` 各 arm 归一为 `Option<(ConfigWritePatch, String, SecretString)>`(`0..=2` 预设 / `_` 自定义皆 `Some(...)`,`3` → `login_wps(prompter)?`),`let Some((patch, key_name, key)) = outcome else { return Ok(()) };` 再 `write_config` + `write_credential`;`login_wps`:`select` 方式 → `OAuth2` 打 notice(含「暂不支持」)+ 返回 `Ok(None)` / `WPS CodingPlan` → `login_wps_codingplan` 后 `Ok(Some(...))`,方式 select 取消 → `Err(Cancelled)`(见 design D4/D5)
- [x] 3.3 【重构】清理

## 4. 全量校验

- [x] 4.1 `cargo build` 通过、`cargo clippy --all-targets -D warnings` 零警告
- [x] 4.2 `cargo test` 全绿(新增 §2 / §3 + 既有测试不回归)
- [x] 4.3 手动冒烟:`cargo run -- auth login` →「WPS AI → OAuth2」见「暂不支持」提示且不写;再「WPS AI → WPS CodingPlan → 协议 / 模型 / key」→ 检查 `~/.config/mysteries/config.toml` 的 `provider` 段(`id=wps`、`kind`、`base_url`、`model`)与 `credentials` 的 `wps` 行
- [x] 4.4 `openspec validate add-wps-ai-provider --strict` 通过
