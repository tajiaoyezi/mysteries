# tool-system Delta

## ADDED Requirements

### Requirement: Tool::plan_only 与 mode-aware schema 下发(schema-omit)

`Tool` SHALL 提供 `fn plan_only(&self) -> bool`(default `false`);标记「仅 Plan 模式有意义」的工具(如 `submit_plan`)override 为 `true`。`ToolRegistry` SHALL 提供 `schemas_for(mode: PermissionMode) -> Vec<schema>`:
- **`Plan` 模式**:仅含 `permission_level()==ReadOnly || plan_only()` 的工具(只读研究工具 + plan_only 工具),摘掉 `Edit`/`Execute` 类(schema-omit)。
- **非 `Plan` 模式**:仅含 `!plan_only()` 的工具(全部除 plan_only 类——plan_only 工具在别模式无意义、不下发)。

两路 MUST 维持既有插入顺序。既有 `schemas()`(不分 mode)行为 MUST 不变(behavior-preserving)。`plan_only` 默认与 `schemas_for` 过滤为 headless 纯逻辑,强制 TDD。

#### Scenario: Plan 模式摘变更类、留只读 + plan_only

- **WHEN** registry 依次含 ReadOnly / Edit / Execute / plan_only 各一,取 `schemas_for(Plan)`
- **THEN** 仅含 ReadOnly 与 plan_only 两项(Edit/Execute 被摘),顺序保持

#### Scenario: 非 Plan 模式摘 plan_only

- **WHEN** 取 `schemas_for(Normal)`(或 AcceptEdits / Yolo)
- **THEN** 含 ReadOnly / Edit / Execute,不含 plan_only 项

#### Scenario: plan_only 默认 false

- **WHEN** 查一个未 override 的普通工具的 `plan_only()`
- **THEN** 为 `false`
