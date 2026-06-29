# 2026-06-29 · 28 · archive simplify-wps-auth-default-model

## 决策

- **WPS CodingPlan auth 去掉「选模型」步、写默认模型** | 主导:用户(贴 `auth login` 截图,要「auth 只认 provider、具体模型进 TUI 用 `/model`·`/models` 切」)| 让 WPS 与其它三预设「只填 key、模型默认」对齐
- **D1 去 model select、`model = WPS_DEFAULT_MODEL`** | 模型切换交给现成 `/model <name>` 与后续 `/models` | 弃:保留 select(用户明确不要、且与其它预设不一致)
- **D2 默认 `WPS_DEFAULT_MODEL = "zhipu/glm-5.2"`**(实现常量,用户拍板;随需改常量 + 单测,不钉 spec 字面)
- **D3 移除孤立的 `WPS_MODELS`** | 去 select 后无引用 → 清自己产生的孤儿;`/models` epic Change 2 以 per-provider registry 重新引入 | 弃:`#[allow(dead_code)]` 留着(脏、clippy `-D warnings` 不过)
- **D4 测试调整** | `login_wps_codingplan` 的 `select` 减为 1 次(协议);删 `selects_nth_model` / `cancelled_at_model`;断言 `WPS_DEFAULT_MODEL`;`run_auth_login_wps_*` 读 `raw.providers["wps"].model`(Change 1 多 provider schema 落地的连带)
- **非新路径(改既有受测函数),无红灯停点**;红→绿→重构连做,完成交回复核

## 变更

- `src/cli.rs`:+`WPS_DEFAULT_MODEL = "zhipu/glm-5.2"`;`login_wps_codingplan` 去掉模型 `select`(只剩协议 select + key)、`model = WPS_DEFAULT_MODEL`;**移除 `WPS_MODELS`**;更新相关测试
- 验证:`cargo test` 292 lib + 1 e2e passed / 2 ignored;`cargo clippy --all-targets -D warnings` 零警告(`WPS_MODELS` 无残留);`openspec validate --strict` 过;冒烟(隔离 USERPROFILE)—— 「Select model」步消失,`config.toml` 的 `[providers.wps].model = "zhipu/glm-5.2"`
- archive:`changes/simplify-wps-auth-default-model` → `changes/archive/2026-06-29-simplify-wps-auth-default-model`;`specs/cli-runtime` MODIFIED「WPS AI provider 登录(协议 + 模型选择)」(去交互选模型、改写默认模型)

## 待决

- **`/models` epic**:② provider 注册表(内置模型目录,四家)+ 运行时热切;③ `/models` TUI 模态 picker
- **(调研已完成、答复待综合,未开 change)** 用户问的路线图可行性:`1.2 持久化`(技术方案 §13 = `SessionStore` trait file/sqlite,会话/历史落盘,post-1.0)、`1.3 权限工效`(§13 = `PolicyEngine` allowlist/风险分级/always-allow,**非** Claude 的 accept-edits/plan/auto/yolo 模式)、输入历史(路线图 1.4)、Ctrl+Enter 换行(Windows/crossterm 可行;**Shift+Enter 在 Windows ConPTY 不可靠**,不建议)—— 已并行调研,结论待综合后由用户定是否开 change

## 引用

- change:`simplify-wps-auth-default-model`(D1–D4 见 design.md;archive 路径 `changes/archive/2026-06-29-simplify-wps-auth-default-model`)
- 前置 change:`add-wps-ai-provider`(26,WPS auth 流程)、`add-multi-provider-config`(27,config 多 provider schema)
- session 主导:用户贴 `auth login` 截图提「auth 只认 WPS、模型用 `/models` 切」→ 小 change(去 model select + 默认 `zhipu/glm-5.2` + 清 `WPS_MODELS`)→ 子 agent implement(无停点)→ 主 agent review(独立 test/clippy、核去 select 与 `WPS_MODELS` 清除、§ 连带读 providers map)
