## MODIFIED Requirements

### Requirement: --resume 恢复会话

CLI SHALL 支持 `--resume`(无参):启动进 TUI 后弹**会话选择 modal**(`SessionPicker`,见 `tui-shell`),列出历史会话(短 id / 时间 / 首条 `User` 摘要,mtime 逆序);用户选中一个 → **运行时 hot-swap** 该会话——`SessionStore::load(id)` 得 `(meta, history, transcript)`,`history` 首条 `System` 消息 SHALL 替换为当前 `DEFAULT_SYSTEM_PROMPT`(仅还原对话内容),再按本 delta「恢复会话前收口旧中断残留」执行 activation normalization 后替换运行中的 `agent_history` 与 `transcript`,并以 `meta.provider` / `meta.model` 经 **profile 查找**(取 `ProviderProfile` kind / base_url,非仅 id)还原 provider;`meta.provider` 不在当前配置或凭据缺失时 SHALL 回退 startup 默认 provider 并以 `Notice` 提示、不 panic。除旧中断残留 normalization 这一唯一例外外，System 之后的对话内容 MUST 保持不变。hot-swap 后续落盘 SHALL **续写选中会话文件**(不新建)。用户 `Esc` 取消 → 保持进入时的(新)会话。`sessions/` 无历史会话时 SHALL NOT 弹 picker,按新会话继续。无 `--resume` 时对话流程与现状一致,并为本次运行创建新会话。

#### Scenario: resume 弹列表并 hot-swap 还原

- **WHEN** 存在多个历史会话,以 `--resume` 启动,在 `SessionPicker` 中选中一个含工具调用的会话
- **THEN** `agent_history` 与 `transcript` hot-swap 为该会话经 activation normalization 后的内容,provider / model 取自其 `meta`,后续落盘续写该会话文件

#### Scenario: meta.provider 缺失回退

- **WHEN** 选中会话的 `meta.provider` 不在当前配置中(或其凭据缺失)
- **THEN** 回退 startup 默认 provider、给出 `Notice`,不 panic

#### Scenario: System 消息用当前默认

- **WHEN** hot-swap 的会话 `history[0]` 是建会话当时的旧 `System` 消息
- **THEN** 还原后 `history[0]` 为当前 `DEFAULT_SYSTEM_PROMPT`;其后内容除按规范补齐 dangling tool-call occurrence 外不变

#### Scenario: Esc 取消保持新会话

- **WHEN** `--resume` 弹出 `SessionPicker` 后按 `Esc`
- **THEN** picker 关闭,保持进入时的新会话,不 hot-swap

#### Scenario: 无历史会话不弹

- **WHEN** `sessions/` 下无任何会话文件,以 `--resume` 启动
- **THEN** 不弹 picker,按新会话继续

### Requirement: --continue 续最近会话

CLI SHALL 支持 `--continue`(无参):启动时(进 TUI 前)加载最近会话续跑——`load(latest)` 得原始会话数据,首条 `System` 替换当前 `DEFAULT_SYSTEM_PROMPT`,再按本 delta「恢复会话前收口旧中断残留」执行 activation normalization，以其 `meta.provider` / `meta.model` 还原 provider(缺失回退 + `Notice`),**不弹 picker**。除旧中断残留 normalization 这一唯一例外外，System 之后的对话内容 MUST 保持不变。`--resume` 与 `--continue` 同时给出时 SHALL 以 `--resume` 优先(弹 picker)。`sessions/` 无历史会话时按新会话继续。

#### Scenario: continue 续最近

- **WHEN** 存在历史会话,以 `--continue` 启动
- **THEN** 直接加载并 activation-normalize 最近会话、还原历史与 provider,不弹 picker

#### Scenario: resume 优先于 continue

- **WHEN** 同时传 `--resume` 与 `--continue`
- **THEN** 走 `--resume`(弹 picker)

#### Scenario: continue 的 System 用当前默认

- **WHEN** `--continue` 加载的最近会话 `history[0]` 是旧 `System` 消息
- **THEN** 还原后 `history[0]` 为当前 `DEFAULT_SYSTEM_PROMPT`;其后内容除按规范补齐 dangling tool-call occurrence 外不变

#### Scenario: continue provider 缺失回退

- **WHEN** `--continue` 的最近会话 `meta.provider` 不在当前配置中(或凭据缺失)
- **THEN** 回退 startup 默认 provider、给出 `Notice`,不 panic

## ADDED Requirements

### Requirement: 恢复会话前收口旧中断残留

`SessionStore::load(id)` SHALL 继续逐字段返回磁盘中的原始 `(meta, history, transcript, plan)`，保持 round-trip 与严格解析契约；旧中断残留的修复属于 TUI session activation，不得静默改变 store 层返回值。代码中两处 activation load site——`prepare_session_startup` 的 `--continue` startup，以及 `--resume` picker 选中后的 runtime hot-swap——MUST 在替换运行中 `agent_history` / transcript 及接收新 User 输入前，经同一 `normalize_loaded_session`（或等价单一 seam）收口 history 与 transcript。normalization MUST NOT 在 load 时改写磁盘、改变 session JSONL tag / 字段，或破坏既有 System replacement、provider / model 恢复及 `apply_loaded_plan` 语义。

对 loaded history 中每条含 tool calls 的 Assistant，其后、下一个非 `ToolResult` 消息前的连续结果 SHALL 视为该 Assistant 的结果组。每个已有结果按 id 消费最早未配对 occurrence；已有结果 MUST 保持原内容与顺序。每个剩余 occurrence MUST 按模型顺序在该组末尾、下一个非结果消息之前插入且仅插入一个 `ToolResult{is_error:true, content:"tool call interrupted before completion"}`。实现 MUST 使用 occurrence / FIFO 多重集语义，不得按 id 去重，不得把合成结果跨过后续 User / Assistant 统一追加到 history 末尾。

对 loaded transcript，所有 `ToolCardStatus::Running` 卡 SHALL 在激活前改为 Error、output=`上次会话已中断`、`truncated=false`、`exit=None`；Done / Error 卡与 User / Assistant / Thinking / Notice / Error 等其他 block MUST 逐字段保持不变。恢复后的首次 Provider 请求 MUST 只看到每个历史 tool-call occurrence 均有一个配对结果，且新 turn 复用旧 `call_id` 时 finished 只能更新新建 Running 卡。

normalization MUST 幂等：history 已完整配对且 transcript 已无 Running 时逐字段零变化；同一 loaded session 即使重复经过 seam，也不得重复追加 synthetic result 或改写已经收口的卡。

#### Scenario: raw load 保持原始 round-trip

- **WHEN** 一个旧 session 文件含 dangling `Assistant.tool_calls` 与 Running ToolCard，并直接调用 `SessionStore::load(id)` 而尚未进入 TUI activation
- **THEN** store 返回值与磁盘逐字段一致，文件也未被改写；normalization 只在后续 activation seam 中发生

#### Scenario: continue startup 接线双收口

- **WHEN** 最新 session 的一个 Assistant 组含两个同 id `call-1` occurrence、其后只有第一个正常 ToolResult，transcript 同时含多张 Running 及既有 Done / Error 卡，并以 `--continue` 启动
- **THEN** `prepare_session_startup` 返回给运行时的 history 为第二 occurrence 补一个 interrupted ToolResult；全部历史 Running 卡已变 Error / “上次会话已中断”且重置 `truncated` / `exit`，Done / Error 与其他 block 不变；首次 Provider 请求无 dangling call

#### Scenario: resume picker hot-swap 接线双收口

- **WHEN** 以 `--resume` 启动并在 picker 选中同类旧 session，触发 runtime hot-swap
- **THEN** 在替换共享 `agent_history` / transcript 与接收新 Prompt 前执行与 continue 相同的 normalization；新 turn 再 started / finished `call-1` 时只更新新卡，历史卡保持 Error，首次 Provider 请求无 dangling call

#### Scenario: 合成结果不跨越后续消息

- **WHEN** loaded history 中一个 dangling Assistant 结果组之后还存在 User / Assistant 消息
- **THEN** 合成 interrupted ToolResult 插在该结果组末尾、后续非结果消息之前；不得统一追加到 history 尾部或改写已有配对结果

#### Scenario: 已完整会话重复 normalization 幂等

- **WHEN** history 的每个 tool-call occurrence 已各有一个结果且 transcript 无 Running 卡，并连续两次执行 activation normalization
- **THEN** 第二次输出与第一次逐字段相等，不追加结果、不改变 Done / Error 卡或其他 transcript block
