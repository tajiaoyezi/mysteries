# readonly-subagent Specification

## Purpose
定义`delegate_task`的只读subagent委派契约：每次有效调用以当前Provider/model snapshot创建独立临时child Agent，将能力限定为canonical workspace root内的四个本地读取工具，并以固定depth/iteration/deadline、有界并行与按occurrence发布、untrusted bounded envelope保证预算、终止、结果与session隔离。

## Requirements

### Requirement: 只读任务委派与独立child上下文

系统 SHALL 在TUI与headless产品入口提供内置`delegate_task`，接收唯一必填的非空字符串`task`，并为每次有效调用创建一个临时child Agent。空白判定 MUST 使用`task.trim().is_empty()`语义；通过后不得裁剪或改写原task。child MUST 在调用时原子取得parent当前同一份Provider/model snapshot与`ToolContext.cwd`，并在该次invocation全程固定使用该snapshot；使用既有Agent Loop、固定Normal permission mode与`Depth::Low`；其初始history MUST 恰为`System(SUBAGENT_SYSTEM_PROMPT), User(task原字符串)`，不得复制parent history、System、thinking、permission mode、plan、session metadata或先前tool results。

#### Scenario: 有效任务使用当前runtime并返回报告
- **WHEN** parent经原子pair switch切换到新的Provider/model，再调用`delegate_task`执行非空调研任务
- **THEN** child的首个Provider请求只发往新Provider、model字段为新model、messages恰为child System与该task，最终文本作为parent当前tool occurrence的结果返回

#### Scenario: 运行中的child不随parent runtime迁移
- **WHEN** child首轮请求取得snapshot后，parent runtime被原子切换为另一Provider/model，而同一child继续第二轮
- **THEN** 该child第二轮仍使用首次取得的旧Provider/model，下一次新`delegate_task` invocation才使用新pair

#### Scenario: 空白任务在Provider前失败
- **WHEN** `task`缺失、不是字符串、为空或只含空白
- **THEN** outer delegate started/finished仍各一次并返回稳定`is_error`结果，但不得创建child scope、发出child observer事件、调用Provider或执行任何child工具

#### Scenario: 四种权限模式均可只读委派
- **WHEN** parent分别在Normal、AcceptEdits、Yolo与Plan中生成`delegate_task`
- **THEN** schema均可见且调用不弹权限框；child始终以Normal运行、没有Plan transient instruction，能力在四种parent模式下均保持同一只读上限

### Requirement: child能力精确收窄且不可递归

child `ToolRegistry`与execution capability中的tool names MUST 恰为`list_dir`、`read_file`、`glob`、`grep`并保持root registry顺序，permission levels MUST 恰为`ReadOnly`。child registry MUST 结构性排除Network、Edit、Execute、`submit_plan`、`update_plan`、`ask_user`与`delegate_task`；schema隐藏、registry lookup、scope dispatch clamp与permission gate MUST 共同fail-closed。产品root只允许一层child，child的remaining child depth MUST 为0。

#### Scenario: child收到精确的四工具schema
- **WHEN** `delegate_task`启动child首轮Provider请求
- **THEN** tools字段按顺序恰含`list_dir/read_file/glob/grep`，不含其他root或交互工具

#### Scenario: 模型硬发隐藏工具不能绕过
- **WHEN** child Provider硬发`web_fetch`、`write_file`、`run_shell`、`ask_user`、`update_plan`或`delegate_task`
- **THEN** 每个occurrence均得到is_error ToolResult，permission decider、Network preview、用户交互与目标execute调用次数均为0

#### Scenario: child不能再次派生
- **WHEN** depth为0的child尝试调用或派生另一个child
- **THEN** 操作fail-closed，parent与sibling scope保持可运行且未被取消

### Requirement: child读取限定在canonical workspace root

child MUST 把调用时`ToolContext.cwd` canonicalize为workspace root，并把它作为四个读取工具的强制边界。四个工具 MUST 在同一blocking worker/permit内canonicalize目标、验证目标等于root或为其descendant，并使用该canonical target执行实际I/O；不得在async runtime线程做blocking path解析，也不得验证后重新解析原始输入。目录walker加载的parent、`.ignore`与`.gitignore`规则文件也受同一边界约束：read root外ancestor规则不得读取或影响结果，read root内规则文件canonical后越界必须在解析前fail-closed；preflight MUST 按既有ignore/hidden pruning逐层验证规则文件，被剪枝目录自身可能加载的control file仍验证，但不得继续探测actual walker不可达的descendant。read root内parent/nested规则的precedence与whitelist语义 MUST 保持既有行为，但parent规则命中调用者显式target或其ancestor时不得在到达该target前将其剪枝。target等于root只表示通过containment，不改变底层工具的输入类型：三个目录工具可在root目录执行，`read_file`成功case使用root下文件，而`read_file(root目录)`继续返回既有目录读取错误。在单次调用期间filesystem namespace未被并发修改的威胁模型下，绝对workspace外路径、`..`逃逸及symlink/junction解析到workspace外均 MUST 返回is_error，目标内容不得进入child history或后续Provider请求。普通root Agent未携带child read root时 MUST 保持既有绝对路径与gitignore行为。

#### Scenario: workspace内绝对与相对路径可读
- **WHEN** child分别以workspace内相对路径和canonical absolute路径读取同一文件
- **THEN** 两次调用均成功且内容一致

#### Scenario: 绝对路径和父目录逃逸被拒绝
- **WHEN** child以workspace外absolute路径或解析后越过workspace root的`..`路径调用任一读取工具
- **THEN** 调用返回is_error，外部内容未出现在ToolResult、child history或Provider请求中

#### Scenario: symlink或junction逃逸被拒绝
- **WHEN** workspace内symlink或Windows junction指向workspace外目标，child通过该入口读取或遍历
- **THEN** canonical containment检查拒绝调用且不访问目标内容；平台无法创建对应链接时测试必须明确记录skip原因

#### Scenario: workspace外parent ignore不影响child
- **WHEN**workspace的ancestor含会隐藏workspace内probe的`.ignore`或`.gitignore`
- **THEN**child目录工具仍能看到probe，外部规则文件不被读取；普通root Agent继续保持既有parent ignore语义

#### Scenario: ignore规则文件链接越界被拒绝
- **WHEN**workspace内`.ignore`或嵌套`.gitignore`链接到workspace外规则文件
- **THEN**child在规则解析和actual walk前返回containment is_error，外部marker不进入ToolResult、history或Provider请求；平台无法创建file symlink时测试记录原始OS skip原因

#### Scenario: root Agent路径兼容
- **WHEN** 未配置child read root的普通root Agent按变更前方式读取workspace外absolute路径
- **THEN** `resolve_path`与ToolOutcome行为逐字段保持不变

### Requirement: child预算与终止方向固定

产品root MUST 以remaining child depth 1运行；有效参数校验后、任何filesystem或Provider工作前 MUST capture `invocation_time`与runtime snapshot并派生child scope。每个child scope MUST 使用`max_iterations=min(parent.max_iterations, 8)`、`deadline=min(parent.deadline, invocation_time+120s)`与remaining child depth 0；该deadline覆盖进程级blocking permit等待、workspace-root canonicalization、child构造与完整run。workspace preflight MUST 与四个fs工具共用同一进程级blocking limiter，owned permit在真实blocking closure结束前不得释放；取消awaiting future不得提前释放已移入closure的permit。parent cancellation/deadline MUST 传播到所有in-flight child；child自身deadline或失败 MUST 只结束该delegate occurrence，不得取消parent或sibling。

#### Scenario: 固定预算正确派生
- **WHEN** parent iteration预算分别为4和20且无deadline
- **THEN** child预算分别为4和8、deadline为调用时刻后120秒且depth为0

#### Scenario: parent更早deadline优先
- **WHEN** parent deadline早于调用时刻后120秒
- **THEN** child继承parent较早deadline，不得移除或推迟它

#### Scenario: parent取消传播并无迟到事件
- **WHEN** child停在Provider或读取工具等待点时parent被取消
- **THEN** child future完成取消，outer delegate occurrence由parent Loop收口为恰一个synthetic termination ToolResult，termination后无child finished、usage、Idle或文本事件

#### Scenario: child deadline只形成工具错误
- **WHEN**虚拟时钟推进至child 120秒deadline而parent未终止
- **THEN** `delegate_task`返回稳定is_error ToolOutcome，parent继续下一轮Provider，且不产生全局Interrupted

#### Scenario: workspace preflight也受child deadline约束
- **WHEN** workspace root canonicalization停在受控blocking点，parent仍active且child 120秒deadline到达
- **THEN** `delegate_task`返回child-only deadline ordinary error、Provider调用为0，awaiting preflight future被drop且迟到canonicalization结果不得启动child；已启动closure继续占用blocking permit直到真实结束

#### Scenario: 多波取消不堆积detached preflight
- **WHEN** 首批4个workspace canonicalization closure均停在受控blocking点，其delegate awaiting future因parent终止被drop，随后新turn再次发起4个delegate
- **THEN** 新preflight在旧closure释放permit前不得进入blocking closure，跨两批preflight与四fs工具合计的真实blocking max-active始终不超过4

### Requirement: 连续委派有界并行且按occurrence发布

`delegate_task` MUST 以`ParallelSafe`参加既有Agent安全批次，并与相邻的其他`ParallelSafe + ReadOnly + !plan_only`调用处于同一eligible segment；outer segment最多4个tool future同时in-flight，第五项及以后work-conserving等待。该上限不限制单轮delegate occurrence总数。物理完成 MAY 乱序，但outer ToolResult、observer finished与下一轮Provider可见顺序 MUST 严格按模型原始occurrence，重复call id不得去重。非eligible工具仍形成既有屏障。

#### Scenario: 第五个child等待空位
- **WHEN** 同一回复含5个连续delegate calls且前4个均在受控Provider等待点保持in-flight
- **THEN** active child峰值为4，第5个在任一前项释放后才进入

#### Scenario: 乱序完成仍按occurrence发布
- **WHEN** 两个delegate calls按第二个先完成、首个后完成的顺序物理结束
- **THEN** parent history、observer finished与Provider下一轮看到的两个ToolResult仍按首个、第二个occurrence排列

#### Scenario: 重复call id不合并
- **WHEN** 两个delegate occurrence使用相同call id
- **THEN** 系统仍执行并发布两个独立ToolResult，每个occurrence恰好一个

#### Scenario: exclusive工具保持屏障
- **WHEN**模型回复按`read_file, delegate_task批次, run_shell`排列
- **THEN** `read_file`与相邻delegate属于同一eligible segment且合计最多4个outer future同时active；`run_shell`必须等待该segment全部按occurrence发布后才启动

### Requirement: child结果有界、标记不可信且不持久化内部状态

成功raw content MUST 精确构造为`subagent report (untrusted):\n{child_final_text}`；空最终文本 MUST 视为错误。系统 MUST 先形成完整raw envelope，再按`ToolContext.max_output_bytes`在UTF-8边界整体截断并正确设置`truncated`，`exit`为None；cap足够时成功content以完整固定前缀和换行开头，cap小于前缀长度时只返回raw envelope的UTF-8安全前缀片段且不得越过前缀泄漏child文本。仅由`DelegateTaskTool`在parent仍active时返回的ordinary error使用`delegate_task failed: {reason}`raw格式并应用同一bounded规则；required-depth/unknown/scope dispatch拒绝保持既有scope错误，parent cancellation/deadline保持outer synthetic termination，二者均不得使用delegate error前缀。所有错误不得包含parent history、child thinking或未公开工具结果。session只持久化outer ToolCall/ToolResult及标准工具卡，不得写child history、run identity、scope、内部工具卡或单独child session。

#### Scenario: 成功报告带固定envelope
- **WHEN** child返回非空最终文本且`max_output_bytes`足以容纳固定前缀
- **THEN** outer ToolOutcome为`is_error=false`、`exit=None`，content以固定untrusted前缀开头且只含最终报告

#### Scenario: UTF-8安全截断
- **WHEN** envelope加child文本超过`max_output_bytes`且截点落在多字节字符中
- **THEN** content在前一个有效UTF-8边界结束、`truncated=true`且不panic

#### Scenario: 极小cap只截取envelope前缀
- **WHEN** `max_output_bytes`小于success或error固定前缀自身的字节数
- **THEN** content长度不超过cap、是对应raw envelope的UTF-8安全前缀、`truncated=true`，且success结果不得包含任何child文本

#### Scenario: outer拒绝与终止不伪装成delegate错误
- **WHEN** depth不足的scope硬发delegate，或parent cancellation/deadline覆盖尚未发布的delegate outcome
- **THEN** 前者返回既有scope dispatch错误，后者只生成既有synthetic termination；两者均不含`delegate_task failed:`且不得重复ToolResult

#### Scenario: child内部状态不进入session
- **WHEN** 含成功或失败delegate occurrence的parent turn被保存并经`--continue`或`--resume`加载
- **THEN** 只恢复outer call/result/card，child messages、identity、内部工具与scope均不存在且不会被重跑
