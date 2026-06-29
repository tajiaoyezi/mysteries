## 1. 去模型 select + 默认模型(TDD —— 改既有受测函数,无红灯停点)

- [x] 1.1 【红】更新 `login_wps_codingplan` 测试:① 协议 `OpenAI` + key(**无模型 select**,scripted select 脚本只剩协议 1 项)→ `(ConfigWritePatch{id="wps", kind=OpenAi, base_url=OpenAI 端点, model=WPS_DEFAULT_MODEL}, "wps", key)`;② 协议 `Anthropic` → `kind=Anthropic`、`base_url=Anthropic 端点`、`model=WPS_DEFAULT_MODEL`;③ 取消:协议步 `select=[None]` / key 步 `secret=[None]` → `Cancelled`;**删**「选第 k 模型」测试。同步更新 `run_auth_login_wps_codingplan_*` 断言 `model = WPS_DEFAULT_MODEL`、select 脚本去掉模型项。运行确认失败(旧实现仍 select 模型 → 脚本/断言不符)
- [x] 1.2 【绿】`cli.rs` 加 `WPS_DEFAULT_MODEL = "zhipu/glm-5.2"`;`login_wps_codingplan` 去掉模型 `select` 步、`model = WPS_DEFAULT_MODEL.to_string()`;**移除 `WPS_MODELS`** 常量(本 change 后无引用,见 design D3)
- [x] 1.3 【重构】清理

## 2. 全量校验

- [x] 2.1 `cargo build` 通过、`cargo clippy --all-targets -D warnings` 零警告(确认 `WPS_MODELS` 移除后无 dead_code / 无残留引用)
- [x] 2.2 `cargo test` 全绿(更新后的 WPS 测试 + 既有不回归)
- [x] 2.3 手动冒烟:`cargo run -- auth login` →「WPS AI → WPS CodingPlan → 选协议 → 填 key」,**确认不再出现「Select model」步**;检查 `config.toml` 的 `model` = `zhipu/glm-5.2`(用隔离 USERPROFILE,别覆盖本机)
- [x] 2.4 `openspec validate simplify-wps-auth-default-model --strict` 通过
