## Why

当前 TUI 三处交互体验有缺口,影响「读长输出 / 翻历史」的基本可用性:① 工具卡(`设计规范/03` C5)全量渲染 stdout/result,多工具轮把 transcript 撑得很长、最终回答被推远,缺 claude-code 式的折叠;② 键盘只有 `PageUp`/`PageDown`,没有行级 `↑`/`↓` 与 `Home`/`End`,精读与跳顶/回底不顺手;③ Windows Terminal 实测鼠标滚轮收不到/不响应,而滚轮目前是唯一的「连续滚动」手段,一旦失效就只能翻页。本 change 把折叠、键盘滚动键补全、滚轮失效的根因与降级路径一并补齐,让 transcript 在任何终端下都「可折叠、可键盘全覆盖滚动」。

## What Changes

- **A 工具输出折叠 + `ctrl+o` 全局展开**:`TranscriptBlock::Tool(ToolCard)` 默认**折叠为单行**(头行:状态 glyph + 工具名 + args 摘要 + 结果摘要/exit),`ctrl+o` 全局 toggle 展开/折叠**所有**工具卡(claude-code 风格)。新增 `AppState` 折叠态字段 + `ctrl+o` 键路径。本期**只折叠 `Tool` 块**,`Assistant` 长文本不折叠(避免害「最终回答可见」)。折叠细节有**待拍板项**(见 design.md「决策 ①」),spec 先按推荐默认锁定。
- **B 键盘滚动键补全(`↑`/`↓` 行级、`Home` 到顶、`End` 回底并恢复底部跟随)**:在既有 `PageUp`/`PageDown` 上补全 `↑`/`↓`(行级步进)、`Home`(到顶)、`End`(回底 + 恢复 `follows_bottom`)。新增 `scroll_to_top` / `scroll_to_bottom` 原语与 `handle_scroll_key` 新键路径。键位归属(滚动 vs 输入编辑)在 design.md「决策 ②」说清。
- **C Windows Terminal 滚轮无响应:根因 + 诚实降级**:design.md 给出**根因结论**——这是 **ConPTY/Windows 构建相关的平台限制**(非我方配置错误、亦非 crossterm 能力缺陷;详见 design.md「决策 ③」与引用)。**不**硬塞未经验证的「修复」:把 `滚轮` 定位为**尽力而为**(在已转发滚轮的终端/较新 Win11 上仍可用),把 **B 键盘滚动**确立为**与鼠标无关、保证全覆盖**的兜底;另加**环境变量门控的原始事件诊断日志**,供在用户真机 Windows 构建上**核验**滚轮事件是否到达(把「实测收不到」从假设变成可验证事实)。

## Capabilities

### New Capabilities
<!-- 无新增 capability:全部为对既有 tui-shell capability 的 requirement 追加或修订。 -->

### Modified Capabilities
- `tui-shell`:**ADDED**「工具输出折叠与全局展开(`ctrl+o`)」(A)、「键盘滚动全覆盖与鼠标滚轮降级(ConPTY 限制)」(C);**MODIFIED**「transcript 滚动」(在 PageUp/PageDown + 行级 + 鼠标滚轮上,增 `↑`/`↓`/`Home`/`End` 键位与 `scroll_to_top`/`scroll_to_bottom` 原语,B)。
  - 注:既有「按键事件去重(仅 Press)」requirement 文本已泛覆盖 `on_key` / 滚动键处理,`ctrl+o` 折叠 toggle 与新滚动键天然落入其约束,**不**单独 MODIFIED;「仅 Press」由 A 的折叠 requirement 自带场景守住。

## Impact

- **code**(本轮 propose 不改,仅登记后续 implement 触及面):
  - `src/tui/app.rs`:`AppState` 增折叠态字段(如 `tools_expanded: bool`,默认 false=折叠)+ `toggle_tools_expanded`;`scroll_to_top` / `scroll_to_bottom` 原语;`on_key` 内 `ctrl+o` 路径(在文本输入 arm 之前拦截,仅 Press)。
  - `src/tui/mod.rs`:`handle_scroll_key` 增 `Up`/`Down`/`Home`/`End` 分支;(C)`run_tui` 增环境变量门控的原始事件诊断日志(尽力而为,不改主循环语义)。
  - `src/tui/render.rs`:`tool_card_lines` 据折叠态渲染单行折叠 / 全量展开两形态;受影响的既有 insta 工具卡快照需迁移(`tui_tool_card_done` / `tui_timeline_*` / `tui_permission_state` 等含 done 卡的帧)。
  - `src/tui/terminal.rs`:**不改**(鼠标捕获已在;C 的结论正是「捕获已正确开启,问题在平台」)。
- **不受影响**:`run_agent_task` / channel 协议 / agent-loop / headless `run_cli` 全不变(纯 UI 层交互改动,无新 `AgentEvent` / `UserInput`)。
- **设计规范引用 + port/adapt/drop 归类**:
  - 工具卡折叠(A)= **adapt**:`设计规范/03` C5 工具卡定义了头/体/脚与「截断时 `⋯ +N 行已截断`」,但**未**定义「默认折叠 + 全局展开」;本期按技术方案 §13「1.4 TUI 体验 … 纯加法:markdown 渲染、diff 高亮、**折叠**」的扩展缝 adapt 出折叠态(终端版的渐进披露,语义保真)。
  - `↑↓`/`Home`/`End` 键盘滚动(B)= **adapt**:`设计规范/02`「交互 / 键位」明示「滚动 / 翻页 / 输入历史(Up/Down/PageUp)等键位**原型未覆盖;1.0 实现自定,纳入快照后锁定**」——本 change 即在此授权缝内补全键位。
  - `ctrl+o` 全局展开(A)= **adapt**:同上,原型未覆盖,属自定键位。
  - 滚轮尽力而为 + 键盘兜底(C)= **adapt**:`设计规范/02` 滚动语义在 TUI 既有 port 为 PageUp/PageDown + 滚轮;本 change 据 ConPTY 平台事实 adapt 为「键盘全覆盖 + 滚轮尽力而为」,无 drop(滚轮管线保留)。
- **deps**:零新增(`crossterm` event/key/mouse 均已在依赖内;诊断日志用 std `fs`)。
