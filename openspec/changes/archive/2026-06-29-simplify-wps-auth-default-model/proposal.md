## Why

WPS CodingPlan auth 现在多一步「交互式选模型」,但用户希望 `auth login` **只认 provider(选协议 + 填 key)**,具体用哪个模型进 TUI 后再用 `/model`(现成)/ `/models`(后续 epic)切换 —— 与 OpenAI / Anthropic / DeepSeek 预设「只填 key、模型用默认」对齐。

## What Changes

- WPS CodingPlan auth **去掉交互式「选模型」步**:流程变为「选协议 → 填 key」;`model` 写入**默认模型实现常量** `WPS_DEFAULT_MODEL`(= `zhipu/glm-5.2`)。
- 移除被本 change 孤立的 `WPS_MODELS` 目录常量(模型目录将在 `/models` epic 的 Change 2 以 per-provider registry 形式重新引入)。
- 模型切换经现成 `/model <name>`(8 个目标仍合法);好看的 `/models` ↑↓ picker 是 epic 第 ③ 步。

## Capabilities

### New Capabilities

(无)

### Modified Capabilities

- `cli-runtime`:**MODIFIED**「WPS AI provider 登录(协议 + 模型选择)」—— WPS CodingPlan 去掉交互选模型、改写默认模型常量;OAuth2 占位、协议选择、key 隐藏读、取消不留半配置等其余契约不变。

## Impact

- **代码**:`src/cli.rs` —— `login_wps_codingplan` 去掉模型 `select`、`model = WPS_DEFAULT_MODEL`;新增 `WPS_DEFAULT_MODEL` 常量;**移除 `WPS_MODELS`**(本 change 后无引用);更新相关测试(去模型 select 脚本、断言 `model = WPS_DEFAULT_MODEL`、删「选第 k 模型」测试)。零新依赖、不触网、不改装配层。
- **UI**:复用既有 auth 交互(协议 select + key read),无 ratatui 改动,无 `设计规范/` 增量。
- **测试**:cli 内核 TDD(mock `AuthPrompter`)。
- **不做(后续 epic)**:`/models` registry / 内置模型目录(Change 2)、`/models` TUI picker(Change 3)。
