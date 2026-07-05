# session-persistence Delta

## ADDED Requirements

### Requirement: 会话快照落盘

系统 SHALL 提供 `SessionStore`,把一次会话落盘为 `<root>/sessions/<uuid>.jsonl`——行式 tagged record:首行 `Meta`(`SessionMeta`),其后 `agent_history` 逐条 `Msg`、`transcript` 逐块 `Block`,每行一个 `serde_json` 序列化的 `SessionLine`。每次保存 SHALL **全量重写**整文件(使 compact 等历史缩短能被反映);`sessions/` 目录不存在时 SHALL 先创建。

#### Scenario: 写入含工具调用的会话

- **WHEN** `write(meta, history=[System, User, Assistant{tool_calls}, ToolResult, Assistant], transcript=[User, Tool, Assistant])`
- **THEN** 文件逐行含 1 个 `Meta` + 5 个 `Msg` + 3 个 `Block`,每行为合法 JSON

#### Scenario: 全量重写反映 compact

- **WHEN** 先以 10 条 history `write`,再以 compact 后的 4 条 history `write` 同一会话
- **THEN** 文件仅含 4 条 `Msg`(旧 6 条不残留)

### Requirement: 会话加载与还原

`SessionStore::load(id)` SHALL 逐行解析,按 tag 分派回 `(SessionMeta, Vec<Message>, Vec<TranscriptBlock>)`,与写入前逐字段一致(round-trip)。分派 MUST NOT 依赖 `Meta` / `Msg` / `Block` 的行间顺序。任一行非法 JSON 或未知 tag → SHALL 返回 `Err`(不静默跳过)。文件 SHALL 恰含一个 `Meta` 行;零个或多于一个 `Meta` → SHALL 返回 `Err`。

#### Scenario: round-trip 完整还原

- **WHEN** `write` 后 `load` 同一 id
- **THEN** 得到的 `meta` / `history` / `transcript` 与写入值逐字段相等

#### Scenario: 行序无关分派

- **WHEN** 会话文件里 `Meta` / `Block` / `Msg` 行交错排列
- **THEN** `load` 仍把各行正确归入三容器

#### Scenario: 损坏行报错

- **WHEN** 会话文件含一行非法 JSON 或未知 tag
- **THEN** `load` 返回 `Err`,不返回部分结果

#### Scenario: Meta 行缺失或重复报错

- **WHEN** 会话文件零个 `Meta` 行,或含多于一个 `Meta` 行
- **THEN** `load` 返回 `Err`

### Requirement: 最近会话查找

`SessionStore::latest()` SHALL 返回 `sessions/` 下修改时间最新的 `.jsonl` 对应的会话 id;空目录或无 `.jsonl` → `None`;非 `.jsonl` 文件 SHALL 忽略。

#### Scenario: 取 mtime 最新

- **WHEN** `sessions/` 有多个 `.jsonl`、mtime 不同
- **THEN** `latest` 返回 mtime 最大者的 id

#### Scenario: 空目录

- **WHEN** `sessions/` 下无 `.jsonl` 文件
- **THEN** `latest` 返回 `None`

#### Scenario: 忽略非 jsonl 文件

- **WHEN** `sessions/` 下混有非 `.jsonl` 文件与 `.jsonl` 会话
- **THEN** `latest` 只在 `.jsonl` 中取最新,忽略其他文件

### Requirement: uuid 会话标识

新会话 id SHALL 由 `uuid` v4 生成;会话文件名 SHALL 为 `<id>.jsonl`;`SessionMeta.id` SHALL 与文件名 id 一致。

#### Scenario: 新会话 id 合法且唯一

- **WHEN** 连续生成两个 session id
- **THEN** 两者均为合法 uuid 且互不相等

#### Scenario: meta.id 与文件名一致

- **WHEN** 新建会话并 `write`
- **THEN** 文件名为 `<id>.jsonl`,`Meta` 行的 `id` 与文件名 id 相同

### Requirement: --resume 恢复会话

CLI SHALL 支持 `--resume`(无参):启动时加载最近会话,以其 `agent_history` 替代默认 seed、还原 `transcript`,并以 `meta.provider` / `meta.model` 还原 provider 续跑。还原 provider SHALL 经 **profile 查找**(按 `meta.provider` 取 `ProviderProfile` 的 kind / base_url 重建),而非仅凭 id 字符串。加载的 `agent_history` 首条 `System` 消息 SHALL 替换为当前 `DEFAULT_SYSTEM_PROMPT`(仅还原对话内容,不锁旧 system prompt)。`meta.provider` 不在当前配置、或其凭据缺失时 SHALL 回退 startup 默认 provider 并以 `Notice` 提示,不 panic。无 `--resume` 时对话流程与现状一致,并为本次运行创建新会话。

#### Scenario: resume 还原历史与视觉

- **WHEN** 存在一个含多轮对话(含工具调用)的会话,以 `--resume` 启动
- **THEN** `agent_history` 与 `transcript` 还原为该会话内容,provider / model 取自 `meta`

#### Scenario: meta.provider 缺失回退

- **WHEN** `--resume` 的会话 `meta.provider` 不在当前配置中(或其凭据缺失)
- **THEN** 回退 startup 默认 provider、给出 `Notice`,启动不中断、不 panic

#### Scenario: System 消息用当前默认

- **WHEN** `--resume` 加载的会话 `history[0]` 是建会话当时的旧 `System` 消息
- **THEN** 还原后 `history[0]` 为当前 `DEFAULT_SYSTEM_PROMPT`,其后 `User` / `Assistant` / `ToolResult` 对话内容不变

### Requirement: 落盘容错不阻断对话

每轮保存失败(IO 错误)SHALL NOT 中断对话:核心 agent 循环继续,失败以 `Notice` 呈现,`agent_history` 不受影响。

#### Scenario: 写失败仅提示

- **WHEN** 某轮保存时 `SessionStore::write` 返回 IO `Err`
- **THEN** 该对话轮正常完成,`transcript` 追加一条 `Notice`,`agent_history` 不受影响

### Requirement: UI 类型序列化保真

`TranscriptBlock` / `ToolCard` / `ToolCardStatus` / `StatusSnapshot` SHALL 可 `Serialize` / `Deserialize` 且 round-trip 保值;既有 `Clone` / `Debug` / `PartialEq` 语义 MUST 不变。

#### Scenario: ToolCard round-trip

- **WHEN** 序列化一个含 `exit = Some(_)`、`truncated = true`、`status = Error` 的 `ToolCard` 再反序列化
- **THEN** 得到与原值相等的 `ToolCard`
