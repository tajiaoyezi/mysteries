## MODIFIED Requirements

### Requirement: 命令块渲染(C8 / C9 / notice)

`render` SHALL 渲染命令产出的 transcript 块(`设计规范/03`):C8 帮助块(两列 `cmd` + `desc`,**6 个帮助条目** —— `/login` / `/logout` 随 auth 迁至 CLI `mysteries auth` 移除;条目含 `/model` 查看与 `/model <name>` 切换两行、不含 `/compact`)、C9 快照块(`provider · model · iter X/maxIter · N msgs · cwd · tools: 7`,其中 `tools: 7` 指 7 个内置**工具**、与命令计数无关、**不变**)、notice 块(info / 占位提示,`info.fg` / 框)。带色,复用 `Theme` + `buffer_to_styled`。

#### Scenario: 帮助块与快照块带色快照

- **WHEN** transcript 含一个 C8 帮助块 / 一个 C9 快照块时渲染
- **THEN** 各自 `insta` 带色快照与锁定一致(C8 两列对齐 6 个帮助条目、**不含** `/login` `/logout`;C9 含 provider/model/iter/msgs/cwd/tools 字段)
