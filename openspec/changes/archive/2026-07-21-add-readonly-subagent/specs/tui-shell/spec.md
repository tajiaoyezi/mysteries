## ADDED Requirements

### Requirement: delegate_task复用C5工具卡且隔离child事件

TUI SHALL 按`设计规范/03-组件清单.md` C5复用现有ToolCard展示outer `delegate_task`的running/done/error、task args、untrusted output与truncation；不得新增child面板、内部工具卡、层级树、快捷键或theme token。`ChannelObserver` MUST 依据`RunIdentity.parent_run_id`忽略child status/tool started/tool finished，避免C10活动状态与outer卡被覆盖；child每次普通或forced-final Provider响应的usage MUST 继续计入当前turn token统计。child text/thinking sink不得写入parent transcript。

#### Scenario: Midnight与Daylight标准delegate卡
- **WHEN**分别在Midnight与Daylight主题渲染running、success、error和truncated delegate card
- **THEN**TestBackend + insta快照仅使用既有C5结构与theme token，现有非delegate快照逐字节零churn

#### Scenario: child内部事件不产生UI块
- **WHEN**一个delegate child依次产生CallingModel、两个内部读取工具、普通或forced-final usage与最终文本
- **THEN**transcript只新增outer delegate卡及其最终ToolOutcome，activity不切换为child工具；token统计恰好累加全部child usage且无child文本/工具卡

### Requirement: Interrupt与session恢复只处理outer delegate

产品TUI root SHALL 使用depth 1 execution scope。Interrupt delegate turn时仍按既有root cancellation路径等待Agent Loop收口，并只发送一个Interrupted terminal event；outer running卡转Error，child future与事件停止。保存/恢复只包含outer parent history与标准ToolCard，`--continue`/`--resume`不得恢复、重跑或显示child内部状态。

#### Scenario: delegate运行中Interrupt唯一收口
- **WHEN**child Provider或读取工具已进入受控等待点后用户按Esc
- **THEN**只出现一次“已中断本轮”，outer delegate卡为Error，无迟到child Done/status/text，下一Prompt可正常执行

#### Scenario: 恢复中断后的delegate会话
- **WHEN**中断delegate后退出并分别用`--continue`与picker `--resume`加载
- **THEN**outer卡无Running残留且不重复ToolResult，child不重启，恢复后的首轮Provider正常
