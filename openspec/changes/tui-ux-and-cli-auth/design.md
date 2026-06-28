## Context

现状(已核实):

- `tui/render.rs` 布局自上而下:header(3)/ transcript(min 8)/ 权限框 / gap / **状态行(1)/ 输入框(3)** —— 状态行在输入框**上方**。
- `tui/terminal.rs` `EnableMouseCapture`(进 alternate screen 时)抢鼠标 → 终端原生选择复制失效;`improve-tui-interaction` 已确认滚轮在 Windows Terminal/ConPTY 本就无响应,键盘滚动(↑↓/PageUp/Home/End)全覆盖兜底。
- `builtin-commands`:`parse_command` 解析 `/help|/clear|/model|/status|/exit|/login|/logout|/compact`,但**无命令元数据**(描述/用法)供补全。
- `cli-runtime`:`cli.rs` 只有 `run_cli`/`run_tui`,**无子命令**;`credential-source` / `config` 均**只读**(env/file 读、TOML 读),无写。

权威次序:code / 测试 > spec。

## Goals / Non-Goals

**Goals:** `/` 命令补全;TUI 可复制;状态栏移底;`mysteries auth` 持久化配置 provider/model/key。

**Non-Goals:** 不做 OAuth 登录(§13 OAuth 留后);不做 TUI 内凭据输入(安全,凭据走 CLI);不引新 dep。

## Decisions

### ① 命令补全(点1)

- 触发:输入串以 `/` 起头且无空格(命令名输入中)→ 计算前缀匹配的内置命令候选;非 `/` 或已过命令名 → 不弹。
- 交互:`↑↓` 移高亮、`Tab`/`Enter` 补全选中项、`Esc` 关补全(不清输入)、继续打字过滤;补全后正常提交走既有命令执行。
- 数据:`builtin-commands` 暴露 `command_metadata() -> &[(name, desc, usage)]`(或等价),补全 UI 与执行解析复用同一命令清单(单一真相,避免漂移)。
- 渲染:补全弹层贴输入框(点3 后输入框在上,弹层在其上方/下方择一,设计规范 C 系列就近),`TestBackend` 快照锁定。

### ② 去鼠标捕获恢复复制(点2)

- 去掉 `terminal.rs` 的 `EnableMouseCapture` / `DisableMouseCapture`;删 `MouseEvent` 处理与 `handle_scroll_mouse`(死代码)。
- 代价:Mac/Linux 失去 TUI 内滚轮,但回归终端原生 scrollback 滚动 + 选择复制;Windows 本无滚轮、纯增益。键盘滚动保留不变。
- 这是 `improve-tui-interaction`「滚轮降级」的收尾:既然滚轮实测不可用,索性让位给终端原生能力(复制 > 程序内滚轮)。

### ③ 状态栏移底(点3)

- `render.rs` 布局把「状态行(1)→ 输入框(3)」改为「输入框(3)→ 状态行(1)」(状态栏成为最底一行)。权限框仍内联于输入框上方。
- adapt 设计规范 02:原型状态行在输入框上方;本 change 移至最底(贴 claude code + D2「底部状态行」字面)。迁移受影响快照。

### ④ CLI auth 子命令(点4)

- `main` 分流加 `auth`:`mysteries auth` → 交互式配置流程(非 TUI、纯 stdin/stderr 提示):
  1. 选 provider(`openai` / `anthropic`);2. 输 base_url(可空→默认 endpoint);3. 输默认 model;4. 输 API key(**隐藏输入**)。
- **隐藏输入**:用既有 `crossterm` 临时 raw mode 读取、不回显(零新依赖);读毕恢复。
- **持久化写**:
  - `config-layering` 加 `write_config`(或 `merge_into_user_config`):读现有 user `config.toml`(容忍缺失)→ 改 `provider.kind`/`base_url`/`model` → **保留其他字段**序列化回写(不整文件覆盖)。
  - `credential-source` 加 `FileCredentialSink`(或 `write_credential`):向 `credentials` 文件 upsert 一行 `provider = key`(已存在则替换、否则追加;保留其他 provider 行)。
- **职责分离**:配置(持久化、含敏感)走 CLI auth;TUI `/model` 仅运行时临时切已配 model(不变)。

### ⑤ apply 拆分(一个 change · 两 agent 并行)

- TUI 组(①②③)与 CLI 组(④)改文件不相交(`src/tui/*` + builtin 元数据 vs `cli/main/credential/config/app`),各自 worktree 并行 apply,主 agent merge 两分支入本 change 单一分支后统一 archive。tasks 按组划分。

## Risks / Trade-offs

- **隐藏输入跨平台**:crossterm raw mode 读密钥在 Windows/Unix 行为差异 → 以「读毕必恢复终端态」+ 失败回落(EOF/错误→不写、提示)兜底;不引 rpassword。
- **凭据文件权限**:Unix `0600` 设定;Windows 无 POSIX 权限,以默认 ACL + 文档提示(写在用户配置目录)。
- **配置写丢字段**:必须 read-modify-write merge(非覆盖),以测试锁「写后其他字段保留」。
- **去鼠标捕获致 Mac/Linux 无程序内滚轮**:接受(换复制 + 终端原生滚动;滚轮本已降级)。
- **状态栏移底偏离原型**:adapt,已记;视觉对眼 gate 由用户冒烟确认。
- **补全弹层与权限框/输入框抢位**:补全为瞬态浮层、仅命令名输入期出现,与权限框(授权期)时序不重叠;快照覆盖。
