## ADDED Requirements

### Requirement: 产品root显式开放单层child预算

TUI与headless产品入口 SHALL 为每个用户Prompt创建全能力、remaining child depth为1的root execution scope，以允许`delegate_task`派生单层child。既有公开`Agent::run`、`run_observed`与`root_scope`兼容入口 MUST 继续创建depth 0 root，scope-aware schema MUST 隐藏需要child depth的工具，dispatch MUST 拒绝模型硬发；调用方未显式选择产品委派入口时不得看到或获得child派生能力。所有child scope的remaining child depth MUST 为0。

#### Scenario: TUI与headless可派生一层child
- **WHEN** TUI或headless产品root处理一个有效`delegate_task`
- **THEN** root identity无parent、depth为1，派生child identity指向该root且child depth为0

#### Scenario: legacy wrapper保持depth零
- **WHEN** library调用方继续使用变更前的`Agent::run`、`run_observed`或`root_scope`
- **THEN** root预算depth仍为0，Provider schema不含`delegate_task`，硬发delegate在observer/permission/execute前fail-closed，其他Provider请求、history与observer逐项不变

#### Scenario: child派生不影响sibling
- **WHEN** 同一root并发派生多个depth 0 child且其中一个自行失败或取消
- **THEN** root与其他sibling保持未取消，只有parent cancellation可传播到全部child
