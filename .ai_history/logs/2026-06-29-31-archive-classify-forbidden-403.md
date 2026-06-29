# 2026-06-29 · 31 · archive classify-forbidden-403

## 决策

- **HTTP 403 与 401 分流:401→Auth,403→fatal Transport(带文案)** | 主导:讨论收敛(实测 bug 排查:用户配 wps 模型后报"provider authentication failed",根因之一是 codeplan 对无权限模型返回 403 被 `classify` 与 401 一并归 `Auth`,文案误导去换 key)| 依据:code(`transport::classify` 单一入口,OpenAI + Anthropic 共用)
- **403 文案明确指向"换模型 / 查 key 权限"而非"鉴权失败"** | `Transport("{label} forbidden (403) — 模型无权限或配额,换模型或检查 key 权限")`;断言不含 `authentication` | 弃:沿用 `Auth`(语义错:key 有效但该模型无权限,不该提示重登);弃:归 `Retryable`(403 非瞬时,重试无意义,应 fatal)
- **改动落在共享 `classify`,Anthropic 自动同步** | 选:改单一 `classify` 函数(401/403/429/5xx/其他分支)| 依据:code(此前 epic 已把 OpenAI/Anthropic 错误分类统一到 `transport::classify(failure, label)`),无需两处维护
- **bug 2(wps auth 实测失败)系统排查结论:非 code bug** | 凭据 `wps [file]` 能 resolve、credential-name 注入逻辑(`config.provider.id`)正确、provider 构造正确;auth 失败来自 codeplan 服务端 401/403。主 agent headless 复现得"network error"系沙箱无法触达内网 `ai-kas.kso.net`。**留给用户在内网判别**:`/models` 切到另一 wps 模型——能用 = 默认 `zhipu/glm-5.2` 无权限(403,本 change 让其显示更清楚),全挂 = key 失效(重 auth)。本 change 仅改善 403 的**可读性**,不声称修好 bug 2

## 变更

- `src/provider/transport.rs`:`classify` 的 `Status(403)` 分支从 `Fatal(Auth)` 改为 `Fatal(Transport("… forbidden (403) — 模型无权限或配额 …"))`;401 保持 `Fatal(Auth)`
- `src/provider/openai.rs`:对应单测 `classify_auth_statuses_as_fatal_auth` → 重命名 `classify_401_as_fatal_auth_and_403_as_forbidden_transport`,403 断言改为 match `Transport` 且含 `forbidden (403)`/`模型无权限或配额`、不含 `authentication`
- spec:`openai-transport` MODIFIED 401/403 分类 requirement
- 验证:`cargo test` 320 lib + 1 e2e passed / 2 ignored;`cargo clippy --all-targets -D warnings` 零警告;`openspec validate --strict` 过

## 待决

- **bug 2 判别在用户手上**:内网 `/models` 切另一 wps 模型确认是默认模型 403 还是 key 失效;若前者,考虑把 WPS 默认模型从 `zhipu/glm-5.2` 改为有权限的一个
- 是否需对 429 之外的 5xx 细分(当前 5xx 统一 Retryable)

## 引用

- change:`classify-forbidden-403`(archive 路径 `changes/archive/2026-06-29-classify-forbidden-403`)
- 关联:`simplify-wps-auth-default-model`(28,WPS 默认模型 `zhipu/glm-5.2` 来源)、`add-models-picker`(30,切模型的交互入口——bug 2 判别靠它)
- session 主导:用户报两实测 bug(右下黑带 + wps auth 失败)+「可以一起,给我修复 prompt 我转发」→ 主 agent 系统排查(auth list + 读码 + headless 复现)定位 403 误归类 → 子 agent 实现 → 主 agent 独立 review 通过(读 `transport.rs:56`/`openai.rs` 测试)→ 黑带归 `add-models-picker`、403 归本 change
