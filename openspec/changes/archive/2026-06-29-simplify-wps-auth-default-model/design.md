## Context

`add-wps-ai-provider` 给 WPS CodingPlan auth 设了「选协议 → 选内置模型(8 选 1) → 填 key」三步。用户反馈:auth 时不该选模型,只认 provider 即可,模型进 TUI 后切。这让 WPS 与其它三预设(只填 key、模型默认)对齐,并把模型选择统一交给 `/model` / 后续 `/models`。

## Goals / Non-Goals

**Goals:**
- WPS CodingPlan auth 去掉模型 select 步;写默认模型常量。
- 保持其余 WPS auth 契约不变(OAuth2 占位、协议选择、key 隐藏、取消不留半配置)。

**Non-Goals:**
- 不做 `/models` picker / per-provider registry(epic Change 2/3)。
- 不改协议选择、OAuth2、装配、其它 provider。

## Decisions

- **D1 去模型 select、写默认模型常量。** `login_wps_codingplan` 从「协议 select + 模型 select + key」减为「协议 select + key」,`model = WPS_DEFAULT_MODEL`。模型切换交给现成 `/model <name>` 与后续 `/models`。**备选**:保留 select(弃:用户明确不要,且与其它预设不一致)。

- **D2 默认 `WPS_DEFAULT_MODEL = "zhipu/glm-5.2"`**(实现常量,用户拍板;随需改常量 + 单测,不钉 spec 字面)。

- **D3 移除孤立的 `WPS_MODELS`。** 去掉模型 select 后 `WPS_MODELS` 无引用 → 按「清理自己产生的孤儿」移除;`/models` epic Change 2 会以 per-provider 模型目录(registry)形式重新引入(形态可能不同,故此处不留 `#[allow(dead_code)]`)。**备选**:`#[allow(dead_code)]` 留着(弃:脏,且 clippy `-D warnings` 不过)。

- **D4 测试相应调整。** `login_wps_codingplan` 的 `select` 调用从 2 次(协议+模型)减为 1 次(协议),scripted `AuthPrompter` 脚本相应缩短;断言 `model = WPS_DEFAULT_MODEL`;删「选第 k 模型」测试;取消测试覆盖协议 / key 两步。`run_auth_login_wps_codingplan_*` 断言 model=默认。

## Risks / Trade-offs

- **移除 `WPS_MODELS` 后 Change 2 再引入** = 少量来回,但每个 change 自洽无死代码(可接受;Change 2 的目录大概率是 registry map 而非独立常量)。
- **本 change 非新路径**,是改既有受测函数 → 不设红灯停点(按 TDD 折中档「改既有行为」可连写),完成时交回复核即可。
