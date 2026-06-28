## MODIFIED Requirements

### Requirement: ratatui 四区最小外壳渲染

系统 SHALL 用 ratatui 渲染 `设计规范/02-布局与交互` 的四区布局,自上而下:顶栏(C1,仅品牌 `✦ mysteries  agent · v1.0`)/ transcript(空会话 → C2 欢迎态;有会话 → user/assistant 文本块)/ **输入框(C11,`mysteries ▸ ` + 占位)/ 状态行(C10,cut1 粗 phase:就绪 / 忙 / 等待授权)**——**状态行位于最底、输入框在其上方**(贴 claude code 底部状态栏;adapt 设计规范 02 原型「状态行在输入框上方」)。`PermissionRequired` pending 时,C6 权限框 MUST 内联钉在**输入框上方**。渲染 MUST 可经 `ratatui::backend::TestBackend` 快照验证(`insta`)。

#### Scenario: 欢迎态结构快照

- **WHEN** 空会话状态渲染到 `TestBackend`
- **THEN** 快照自上而下含 顶栏品牌行、C2 欢迎态、输入框占位、**最底状态行**四区结构,且与锁定的 `insta` 快照一致

#### Scenario: 权限态内联框

- **WHEN** 存在一个 pending 的 `PermissionRequired`(工具名 + args)时渲染
- **THEN** 快照在**输入框上方**含 C6 权限框(`▲ 需要授权` + 工具名/args + `[y·允许][n·拒绝]`),与锁定快照一致

## ADDED Requirements

### Requirement: / 命令补全

输入串以 `/` 起头且仍在**命令名输入中**(尚无空格)时,系统 SHALL 渲染命令补全浮层:列出**前缀匹配**的内置命令(名 + 简述),高亮当前选中项。`↑` / `↓` SHALL 移动高亮;`Tab` 或 `Enter` SHALL 以选中命令名补全输入框;`Esc` SHALL 关闭浮层(不清空输入);继续输入字符 SHALL 重新过滤。补全候选 MUST 取自 builtin-commands 的命令元数据(与执行解析同一命令清单,避免漂移)。非 `/` 开头、或命令名已输完(含空格进入参数)时 MUST NOT 显示浮层。

#### Scenario: 输 / 弹前缀匹配候选

- **WHEN** 输入框内容为 `"/co"`
- **THEN** 补全浮层列出前缀匹配的命令(含 `/compact`)及其简述

#### Scenario: Tab 补全选中项

- **WHEN** 浮层高亮候选为 `/compact`,按 `Tab`
- **THEN** 输入框补全为 `"/compact"`,浮层关闭

#### Scenario: 非命令态不弹浮层

- **WHEN** 输入框为 `"/model gpt"`(已进入参数)或 `"hello"`(非 `/` 起头)
- **THEN** 不显示补全浮层

### Requirement: 终端原生复制(不捕获鼠标)

TUI MUST NOT 启用鼠标捕获(`EnableMouseCapture`),以使终端**原生的文本选择 / 复制与 scrollback 滚动**可用。transcript 滚动改由键盘(`↑↓` / `PageUp` / `PageDown` / `Home` / `End`)承担(既有能力,保持不变)。背景:鼠标滚轮在 Windows Terminal / ConPTY 实测不可用(见 `improve-tui-interaction`),放弃程序内鼠标事件以换取终端原生复制为更优权衡。

#### Scenario: 进入 TUI 不置入鼠标捕获

- **WHEN** 构造终端 guard 进入 alternate screen
- **THEN** 终端**未**被置入鼠标捕获模式(`TerminalGuard` setup 不发 `EnableMouseCapture`),用户可用终端原生选择复制;键盘滚动仍生效
