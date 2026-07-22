## ADDED Requirements

### Requirement: Tool提供source-compatible scoped执行入口

`Tool` trait SHALL 保留现有`execute(args, &ToolContext)`必需方法，并新增object-safe async scoped执行入口及默认返回0的`required_child_depth()`元数据。scoped入口接收只读`ToolExecutionContext`，其中包含既有ToolContext、当前AgentExecutionScope、当前AgentObserver与可选canonical read root；default实现 MUST 忽略新增字段并转发到既有`execute`。schema与dispatch MUST 对required child depth使用同一fail-closed判断。新增接口不得要求现有Tool实现、注册API、registry顺序或ToolOutcome类型迁移。

#### Scenario: 旧Tool无需新增方法即可编译
- **WHEN** 外部或测试Tool只实现变更前要求的trait方法
- **THEN** 它继续编译、required child depth为0，scoped dispatch调用其既有execute且行为不变

#### Scenario: override可取得当前scope和observer
- **WHEN** Tool override scoped入口并由指定run执行
- **THEN** 它取得该run的identity/cancellation、同一observer与原ToolContext，不得取得另一并发run的值

#### Scenario: 可选read root不改变root工具
- **WHEN** `ToolExecutionContext.read_root`为None
- **THEN** default与四个fs Tool的路径解析、权限、并发分类和输出均与变更前一致

#### Scenario: 需要child depth的工具被双重clamp
- **WHEN** Tool声明required child depth为1而当前scope depth为0
- **THEN** registry/Agent生成的scope-aware schema隐藏该工具，dispatch硬发也不调用其permission或execute

### Requirement: restricted registry支持临时child精确共享

assembly SHALL 在注册`delegate_task`及交互工具前，从root registry创建四读取工具的restricted view；该view MUST 共享原Tool实例、保持parent顺序并固定用于每次child。后续向root注册的工具不得自动出现在既有child view中。

#### Scenario: child view不随root后续注册扩张
- **WHEN**先建立四工具restricted view，再向root注册delegate与三个交互工具
- **THEN** child view仍恰含原四项且共享它们的Arc实例，新增root工具不可见

#### Scenario: assembly缺少预期读取工具时fail-fast
- **WHEN**构造child view时四个固定名称中任一未知或重复
- **THEN** assembly测试或内部构造立即失败，不得返回部分child registry或静默减少能力
