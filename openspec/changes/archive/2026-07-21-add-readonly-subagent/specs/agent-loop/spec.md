## ADDED Requirements

### Requirement: Agent dispatch统一使用当前scoped工具上下文

Agent Loop SHALL 在串行与ParallelSafe dispatch中为每个tool occurrence构造仅借用既有`ToolContext`、当前`AgentExecutionScope`、当前`AgentObserver`和Agent可选read root的`ToolExecutionContext`，并统一调用tool的scoped执行入口。上下文 MUST 逐future传递且不得写入共享Tool可变状态；两个并发run使用同一registry时不得串scope、observer或read root。

#### Scenario: legacy Tool default转发零回归
- **WHEN** 只实现既有`execute`的Tool分别从串行和ParallelSafe路径执行
- **THEN** scoped default方法恰调用一次原`execute`，args、ToolContext、ToolOutcome、observer顺序与变更前逐字段一致

#### Scenario: 并发run不串scoped上下文
- **WHEN** 两个root scope并发通过同一Tool实例执行，且scope identity、observer与read root均不同
- **THEN** 每个future只观察到自己的四项上下文，取消其中一个不会改变另一个

#### Scenario: child depth不足时schema与dispatch双重拒绝
- **WHEN** scope的remaining child depth小于Tool声明的required child depth
- **THEN** 该Tool不出现在Provider schema；模型硬发时在observer started、permission gate与execute前得到scope error

### Requirement: delegate作为普通outer occurrence参与Loop收口

Agent Loop SHALL 把`delegate_task`的成功或child-only失败作为普通ToolOutcome按既有occurrence规则写入parent history；parent cancellation/deadline仍由outer scope termination优先裁决并生成synthetic结果。串行路径在tool future返回后、调用history/finished observer发布前 MUST 再次检查parent scope。ParallelSafe路径 MAY 把乱序完成项暂存于仅内部可见的ready buffer，但每个item进入连续可发布前缀、即将同步写history/finished observer且中间无`await`时 MUST 再次检查parent scope；只有这次紧邻发布的post-ready checkpoint SHALL 作为该occurrence的publication linearization point。观察到termination时，当前及所有尚未发布的ready outcome均被丢弃并进入synthetic收口；通过后才允许同步发布。连续ParallelSafe delegate calls沿用上限4、ready buffer与模型顺序发布；child future不得绕过permission/mode/unknown-tool屏障或直接写parent history。

#### Scenario: child失败后parent可继续
- **WHEN** child Provider失败或child deadline到达而parent scope仍可运行
- **THEN** parent history加入一个is_error delegate ToolResult并带完整history请求下一轮Provider，不返回全局ScopedAgentError

#### Scenario: parent终止覆盖未发布child结果
- **WHEN** child物理结果ready，但post-ready checkpoint观察到parent cancellation或deadline
- **THEN** ready结果不得写入history或finished observer；若它已乱序暂存于私有ready buffer则必须丢弃，该occurrence按既有termination文案收口且child不得产生迟到finished/usage/Idle

#### Scenario: nested cancellation不能抢先发布ordinary error
- **WHEN** outer termination branch首次poll为Pending，而delegate future随后在同一次poll中观察parent-derived cancellation并返回child cancellation error或其他ready outcome
- **THEN** post-ready checkpoint必须把该结果提升为outer termination，禁止发布`delegate_task failed:`或普通finished事件

#### Scenario: 私有ready buffer不提前线性化
- **WHEN** ParallelSafe批次的后项先ready并进入私有buffer、前项仍未完成，此时parent终止且后项尚未写入history/finished observer
- **THEN** 后项不得因较早ready而视为已发布；它与前项及其余未发布occurrence都按outer synthetic termination收口，不发布普通outcome

#### Scenario: delegate批次保持模型顺序
- **WHEN** 多个delegate futures乱序完成
- **THEN** outer results、observer finished及下一轮Provider messages仍按原occurrence发布，全部结果完成前不得请求下一轮Provider

#### Scenario: child forced-final observer序列完整
- **WHEN** child触及iteration上限并在forced-final Provider响应中返回`usage: Some`
- **THEN** 同一child identity在该请求前收到`CallingModel`、响应后恰收到一次对应usage callback，成功自然终止再收到`Idle`；ChannelObserver可过滤child status但不得漏掉该usage，Provider error、termination或空final失败路径不得发送`Idle`
