## Why

WPS 内部经 ai-kas codeplan 网关提供多家模型(OpenAI / Anthropic 两种协议端点 + 一组内置模型)。团队要在 `auth login` 里一键接入,但现有候选只有 `OpenAI` / `Anthropic` / `DeepSeek` / `自定义` —— 接 WPS 只能走「自定义」手填 base_url + model,繁琐易错。新增专门的 WPS AI 入口,选协议 + 选模型 + 填 key 即可。

## What Changes

- `auth login` provider 候选在 **`自定义` 上方**新增 **`WPS AI`**。
- 选 `WPS AI` → **二级方式子选择**:`OAuth2 登录(暂不支持)` / `WPS CodingPlan`。
  - **OAuth2(占位)**:回车 → 打印 `WPS AI OAuth2 暂不支持，后续考虑支持。` notice、正常退(`Ok`)、**不写任何配置**(后续考虑接入)。
  - **WPS CodingPlan**:① 选协议(`OpenAI` → `/codeplan/v1`、`kind=OpenAi`;`Anthropic` → `/codeplan/anthropic`、`kind=Anthropic`)② 从**内置模型目录**选一 ③ 输 API key(隐藏)→ 写 `provider{ id="wps", kind, base_url, model }` 入 user `config.toml` + 凭据键 `wps` 入 `credentials`。
- 新增实现常量:两个 codeplan base_url + WPS 内置模型目录。
- 任一步取消 / EOF → 不写任何文件(沿用既有「不留半配置」)。
- **不含** `/model` provider+模型切换器(↑↓ 列出已配 provider 及其模型并热切换)—— 需多 provider 配置 schema + agent 运行时换 provider + TUI 模态,**另开独立 change**。

## Capabilities

### New Capabilities

(无)—— WPS 复用既有 `OpenAi`/`Anthropic` kind + 自定义 base_url + 逻辑 id 凭据注入,不引入新 capability。

### Modified Capabilities

- `cli-runtime`:
  - **MODIFIED**「auth 子命令交互式配置」—— provider 候选加入 `WPS AI`(置于 `自定义` 上方),并引出方式子选择。
  - **ADDED**「WPS AI provider 登录(协议 + 模型选择)」—— OAuth2 占位 + WPS CodingPlan(协议/模型/key)的完整契约。
  - `select_provider` / 端到端装配 / 凭据机制 **不变**(WPS 走既有 kind + base_url + `provider.id` 作凭据名注入路径)。

## Impact

- **代码**:`src/cli.rs` —— `run_auth_login` 候选加 `WPS AI`、新增 `login_wps`(方式子选择)+ `login_wps_codingplan`(协议→模型→key)、WPS 常量(两 base_url + 模型目录)、OAuth2 notice。复用 `AuthPrompter` / `write_config` / `write_credential` / `ConfigWritePatch` / `select_provider`,**零新依赖**、不触网、不改装配层。
- **UI**:不新增 / 不修改 ratatui 组件;复用既有 auth `select` / `read_secret` 交互(已由「交互式选择」requirement 覆盖),无 `设计规范/` 增量。
- **测试**:cli 内核走 TDD(mock `AuthPrompter` + tempdir,离线确定性);无新增 ratatui 渲染、无新 insta 快照。
- **不做(本 change)**:`/model` provider+模型切换器(独立 change,先设计多 provider 配置持久化)。
