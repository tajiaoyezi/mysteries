## ADDED Requirements

### Requirement: 注册表拒绝重名工具

`ToolRegistry::register` SHALL 在工具名已存在时返回 `Err`(重名),不覆盖原有工具;名字未占用时返回 `Ok`。既有的按名注册 / 查找 / `schemas()` 行为不变;实现保留 `Vec` 以维持 `schemas()` 的插入顺序(供模型请求的工具顺序确定)。

#### Scenario: 重名注册被拒

- **WHEN** 用一个已注册过的名字再次 `register`
- **THEN** 返回 `Err`,registry 中保留原工具(不被覆盖)

#### Scenario: 唯一名注册成功

- **WHEN** 用一个未占用的名字 `register`
- **THEN** 返回 `Ok`,该工具可被 `get` 查到
