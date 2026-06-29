# 2026-06-29 · 26 · archive add-wps-ai-provider

## 决策

- **`auth login` 新增 `WPS AI` 入口(置于 `Custom` 上方)** | 主导:用户(贴 WPS ai-kas codeplan 网关截图)| 依据:图示两协议端点(OpenAI `/codeplan/v1`、Anthropic `/codeplan/anthropic`)+ 一组内置模型
- **D1 WPS 复用既有 kind + 自定义 base_url + 逻辑 id,不加 `ProviderKind`** | 选:`kind=OpenAi|Anthropic` + codeplan base_url + `id="wps"`,经既有 `select_provider`(`provider.id` 作凭据名注入)构造 | 弃:加 `ProviderKind::Wps`(要改 `select_provider`/装配,WPS 无任何协议差异)
- **D2 `WPS AI` 不走 `ProviderPreset`,独立 `login_wps` + `login_wps_codingplan`** | preset = 固定单 `ConfigWritePatch`(只读 key),WPS 有**协议 + 模型**两步分支
- **D3 协议 → (kind, base_url) 映射;模型从内置目录 select;两 base_url + 8 模型为实现常量**(不钉 spec 字面,随网关变更改常量 + 单测)
- **D4 OAuth2 占位 = notice + `Ok(None)` 不写** | 主导:用户拍板 | 弃:`Err`/`Cancelled`(它非错误/非取消,是「功能未就绪」)| 边界:首启 onboarding 选 OAuth2 → 仍 `MissingField`(选了明确不支持项的可接受结果,本 change 不特殊处理)
- **D5 菜单索引扩展 + Option 归一** | `provider_options=[…,"WPS AI","Custom"]`(idx 3=WPS AI、4=Custom);`run_auth_login` 各 arm 归一 `Option<(patch, key_name, key)>`,`3 → login_wps`、`let Some(..) else return Ok(())` 让 OAuth2 短路不写
- **D6 `/model` provider+模型切换器明确划出本 change** | 主导:用户(把 `/model` 理解为「列已配 provider + 各自模型、↑↓ 切换」)| 经核对:运行时换 provider **不存在**(`agent.set_model` 只改 model 串、provider 在 `assemble_agent` 焊死)+ config 仅存**单** provider + credentials 无 kind/base_url → 需「多 provider 配置 schema + 运行时换 provider + TUI 模态」,定为**独立大 change**,先 ship WPS
- **审查过程(卡点 A,同 25 先例)**:Rust 新符号红灯用 `unimplemented!()` 签名桩(非编译错的运行时红);测试断言用常量(`WPS_CODEPLAN_*_BASE_URL` / `WPS_MODELS[k]`)非字面;`CaptureSelectPrompter` 钉死菜单顺序含 `WPS AI` 在 `Custom` 上方;**无返工项**

## 变更

- `src/cli.rs`:+WPS 常量(2 base_url + 8 模型目录);+`login_wps_codingplan`(协议→模型→key)、+`login_wps`(OAuth2 占位 / CodingPlan);`run_auth_login` 候选加 `WPS AI`(idx 3)+ arm 归一 `Option` + `let Some(..) else return Ok(())`;+9 测试(§2 ×6 / §3 ×3)+ `run_auth_login_custom` 索引 3→4
- 验证:`cargo test` 281 lib + 1 e2e passed / 2 ignored;`cargo clippy --all-targets -D warnings` 零警告;`openspec validate --strict` 过;冒烟(隔离 USERPROFILE)—— OAuth2 见「暂不支持」且不写、CodingPlan 写 `provider{id=wps,kind=openai,base_url=codeplan/v1,model}` + `credentials` 的 `wps` 行
- archive:`changes/add-wps-ai-provider` → `changes/archive/2026-06-29-add-wps-ai-provider`;`specs/cli-runtime` MODIFIED「auth 子命令交互式配置」(候选加 WPS AI)+ ADDED「WPS AI provider 登录(协议 + 模型选择)」

## 待决

- **`/model` provider+模型切换器**(独立 change)—— 先设计:多 provider 配置持久化(config 存 provider 列表)+ agent 运行时换 provider(`Arc<dyn Provider>` 热插)+ TUI 模态 picker(↑↓)
- WPS **OAuth2 实装**(现占位 notice)
- 假设 8 模型在两协议端点**都可用**(网关页端点与模型组分列);若某些模型仅限某协议,改 `WPS_MODELS` 为按协议分组结构
- 内置模型目录 / base_url 硬编码,随网关变更改常量 + 单测(与既有 preset 默认 model 同策略)

## 引用

- change:`add-wps-ai-provider`(D1–D6 全量见 design.md;archive 路径 `changes/archive/2026-06-29-add-wps-ai-provider`)
- 前置 change:`refine-auth-providers`(23,`auth login` / `AuthPrompter` / preset 映射 / 交互式 select)、`add-first-run-onboarding`(25,卡点 A `unimplemented!()` 签名桩裁决先例)
- session 主导:用户贴 codeplan 截图 → brainstorming(协议/模型/OAuth2/id 四决策 + `/model` scope 经架构核对后拆分)→ OpenSpec propose → 子 agent implement(卡点 A 红灯停点)→ 主 agent review(独立跑 test/clippy、核 D1–D6、发现并隔离 stray `pip/` 缓存)
