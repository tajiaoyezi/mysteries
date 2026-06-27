## ADDED Requirements

### Requirement: C6 权限框 diff body(args 派生)

权限框 SHALL 在头(`▲ 需要授权` + 工具名 + args)下渲染**从 `args` 派生**的 diff body(`设计规范/03` C6),不读文件:`write_file` 的 `content` 整段作 add 行;`edit_file` 的 `old_string` 作 del 行 + `new_string` 作 add 行;`run_shell` 显示命令、无 diff。diff 行色 add=`success.fg`(`+` gutter)/ del=`error.fg`(`−` gutter)/ ctx=`text.body`。动作行 `[y · 允许]` / `[n · 拒绝]`,其中 `[n · 拒绝]` MUST 用 `error.fg`(`设计规范/01`「拒绝=error.fg」)。diff 计算 MUST 为可单测的纯函数(`args` → diff 行),不触文件系统。

#### Scenario: write / edit / shell 的 diff 派生(纯函数)

- **WHEN** 对 `write_file{content}` / `edit_file{old_string,new_string}` / `run_shell{command}` 计算 diff
- **THEN** 分别得到 全 add 行 / (del 行 + add 行) / 无 diff(仅命令),不读取任何文件

#### Scenario: 权限框带 diff 的带色快照

- **WHEN** 一个 `edit_file` 的 pending 权限态渲染到 `TestBackend`
- **THEN** 带色快照含 diff body(del=`error.fg` / add=`success.fg`)与动作行 `[n·拒绝]`=`error.fg`,与锁定一致

### Requirement: C7 致命错误框

`render` SHALL 把 `TranscriptBlock::Error`(由 `AgentEvent::Error` 落入,§9 致命路径)渲为致命错误框(`设计规范/03` C7):`error.bg` 底、`error.border` 描边、`error.fg` 文,含标致命的 title(Loop 已终止、不重试)。

#### Scenario: 致命错误框带色快照

- **WHEN** transcript 含一条 `Error(message)` 时渲染
- **THEN** 带色快照含 C7 致命框(error.bg/border/fg + title + message),与锁定一致

### Requirement: transcript 滚动

`AppState` SHALL 维护 transcript 的 `scroll_offset`:默认**跟随底部**(新内容自动到底);手动 **PageUp / PageDown** 滚动;滚到非底部时新内容 MUST NOT 强制拉回底部(保持阅读位置);offset MUST clamp 在 [顶, 底](不越界)。**仅 transcript 滚动**,顶栏 / 状态行 / 输入框 / 权限框固定。offset 逻辑 MUST 可单测。

#### Scenario: 跟随、手动滚、clamp(逻辑可测)

- **WHEN** 在底部时追加新内容 → 仍贴底;PageUp 后追加新内容 → 保持当前位置(不回底);PageUp/PageDown 至边界 → offset clamp 不越顶 / 底
- **THEN** `scroll_offset` 按上述规则变化(纯逻辑断言)

#### Scenario: 滚动后的 transcript 快照

- **WHEN** transcript 行数超视口且 `scroll_offset` 指向中段时渲染
- **THEN** 快照只显对应窗口的 transcript 行,顶栏 / 状态行 / 输入框位置不变

### Requirement: spinner 动画(确定性渲染)

running 工具卡与 `CallingModel` / `ExecutingTool` phase SHALL 显示动画 spinner(帧序列,如 braille,终端不支持则 ASCII fallback),替代静态字符。`render` MUST 仅依据 `AppState.spinner_frame`(当前帧 index)绘制 —— 即给定 state 渲染确定(insta 可锁固定帧);帧推进 `advance_spinner`(index 循环)MUST 为可单测纯逻辑;动画 tick(`run_tui` 的 `interval`)MUST 与 render / 逻辑解耦(不把时间引入 `render` / `AppState`)。Idle / done / error / WaitingForPermission 用静态 glyph。

#### Scenario: 帧推进循环(纯逻辑)

- **WHEN** 连续调用 `advance_spinner` N 次(N = 帧数)
- **THEN** `spinner_frame` 依次 `0→1→…→N-1→0` 循环

#### Scenario: 固定帧确定性快照

- **WHEN** 以某固定 `spinner_frame` 渲染一个 running 工具卡 / busy phase
- **THEN** 快照取该帧对应 spinner 字符,确定可锁(不依赖时间)
