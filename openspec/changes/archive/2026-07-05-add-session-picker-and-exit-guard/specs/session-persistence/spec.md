# session-persistence Delta

## MODIFIED Requirements

### Requirement: --resume 恢复会话

CLI SHALL 支持 `--resume`(无参):启动进 TUI 后弹**会话选择 modal**(`SessionPicker`,见 `tui-shell`),列出历史会话(短 id / 时间 / 首条 `User` 摘要,mtime 逆序);用户选中一个 → **运行时 hot-swap** 该会话——`SessionStore::load(id)` 得 `(meta, history, transcript)`,`history` 首条 `System` 消息 SHALL 替换为当前 `DEFAULT_SYSTEM_PROMPT`(仅还原对话内容),替换运行中的 `agent_history` 与 `transcript`,并以 `meta.provider` / `meta.model` 经 **profile 查找**(取 `ProviderProfile` kind / base_url,非仅 id)还原 provider;`meta.provider` 不在当前配置或凭据缺失时 SHALL 回退 startup 默认 provider 并以 `Notice` 提示、不 panic。hot-swap 后续落盘 SHALL **续写选中会话文件**(不新建)。用户 `Esc` 取消 → 保持进入时的(新)会话。`sessions/` 无历史会话时 SHALL NOT 弹 picker,按新会话继续。无 `--resume` 时对话流程与现状一致,并为本次运行创建新会话。

#### Scenario: resume 弹列表并 hot-swap 还原

- **WHEN** 存在多个历史会话,以 `--resume` 启动,在 `SessionPicker` 中选中一个含工具调用的会话
- **THEN** `agent_history` 与 `transcript` hot-swap 为该会话内容,provider / model 取自其 `meta`,后续落盘续写该会话文件

#### Scenario: meta.provider 缺失回退

- **WHEN** 选中会话的 `meta.provider` 不在当前配置中(或其凭据缺失)
- **THEN** 回退 startup 默认 provider、给出 `Notice`,不 panic

#### Scenario: System 消息用当前默认

- **WHEN** hot-swap 的会话 `history[0]` 是建会话当时的旧 `System` 消息
- **THEN** 还原后 `history[0]` 为当前 `DEFAULT_SYSTEM_PROMPT`,其后对话内容不变

#### Scenario: Esc 取消保持新会话

- **WHEN** `--resume` 弹出 `SessionPicker` 后按 `Esc`
- **THEN** picker 关闭,保持进入时的新会话,不 hot-swap

#### Scenario: 无历史会话不弹

- **WHEN** `sessions/` 下无任何会话文件,以 `--resume` 启动
- **THEN** 不弹 picker,按新会话继续

## ADDED Requirements

### Requirement: --continue 续最近会话

CLI SHALL 支持 `--continue`(无参):启动时(进 TUI 前)加载最近会话续跑——`load(latest)` 得历史,首条 `System` 替换当前 `DEFAULT_SYSTEM_PROMPT`,以其 `meta.provider` / `meta.model` 还原 provider(缺失回退 + `Notice`),**不弹 picker**。`--resume` 与 `--continue` 同时给出时 SHALL 以 `--resume` 优先(弹 picker)。`sessions/` 无历史会话时按新会话继续。

#### Scenario: continue 续最近

- **WHEN** 存在历史会话,以 `--continue` 启动
- **THEN** 直接加载最近会话、还原历史与 provider,不弹 picker

#### Scenario: resume 优先于 continue

- **WHEN** 同时传 `--resume` 与 `--continue`
- **THEN** 走 `--resume`(弹 picker)

#### Scenario: continue 的 System 用当前默认

- **WHEN** `--continue` 加载的最近会话 `history[0]` 是旧 `System` 消息
- **THEN** 还原后 `history[0]` 为当前 `DEFAULT_SYSTEM_PROMPT`,其后对话不变

#### Scenario: continue provider 缺失回退

- **WHEN** `--continue` 的最近会话 `meta.provider` 不在当前配置中(或凭据缺失)
- **THEN** 回退 startup 默认 provider、给出 `Notice`,不 panic

### Requirement: 会话列表枚举

`SessionStore::list_sessions()` SHALL 返回 `sessions/` 下所有 `.jsonl` 的摘要 `Vec<SessionSummary { id, created_at, first_user }>`,按 mtime **逆序**(最新在顶);`first_user` = 该会话首个 `Msg(User(_))` 内容截断(≤ 60 字符),无 `User` 消息则 `None`。损坏文件(非法 JSON / 缺 `Meta`)SHALL **跳过、不整体失败**(列表容错,区别于 `load` 的严格 `Err`);非 `.jsonl` 文件 SHALL 忽略。

#### Scenario: mtime 逆序 + 首 User 摘要

- **WHEN** `sessions/` 有多个 `.jsonl`、mtime 不同
- **THEN** `list_sessions` 按 mtime 逆序返回,每项 `first_user` 为该会话首个 `User` 消息截断

#### Scenario: 损坏文件跳过

- **WHEN** `sessions/` 含一个损坏 `.jsonl` 与若干正常会话
- **THEN** `list_sessions` 跳过损坏项、返回其余正常会话,不整体报错

#### Scenario: 无 User 消息

- **WHEN** 某会话仅含 `Meta` 与 `System`、无 `User` 消息
- **THEN** 其 `first_user` 为 `None`
