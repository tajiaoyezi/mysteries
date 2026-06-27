## 1. C6 diff 计算(强制 TDD)

- [x] 1.1 【红】写 `compute_diff` 测试:`write_file{content}` → 全 Add 行;`edit_file{old_string,new_string}` → Del 行 + Add 行;`run_shell{command}` → 无 diff;**不读文件**;确认失败
- [x] 1.2 【绿】实现 `compute_diff(tool_name, args) -> Vec<DiffLine{kind,text}>`(`AppState` 或 render 模块,纯函数,见 design D1)
- [x] 1.3 【重构】清理

## 2. C6 权限框 diff body 渲染 + n-fix(insta)

- [x] 2.1 【绿】权限框体渲 diff(add=`success.fg`/`+`、del=`error.fg`/`−`、ctx=`text.body`);动作行 `[n·拒绝]`→`error.fg`(n-fix,design D5)
- [x] 2.2 【insta】带色快照:`edit_file` pending 权限态(含 diff body + 动作行配色)

## 3. C7 致命错误框(insta)

- [x] 3.1 【绿】`render` 把 `TranscriptBlock::Error` 绘为致命框(`error.bg`/`border`/`fg` + title,`设计规范/03` C7,见 design D2)
- [x] 3.2 【insta】带色快照:transcript 含 `Error(message)` 的致命态

## 4. transcript 滚动(强制 TDD + insta)

- [x] 4.1 【红】写 `scroll_offset` 测试:底部追加→跟随;PageUp 后追加→保持位置(不回底);PageUp/PageDown 至边界→clamp 不越顶/底;确认失败
- [x] 4.2 【绿】`AppState` 加 `scroll_offset` + 滚动方法(跟随/翻页/clamp,design D3);`render` 据 offset 切 transcript 行窗口(仅 transcript 区)
- [x] 4.3 【insta】带色快照:中段 offset 的 transcript 窗口(顶栏/状态行/输入框位置不变)
- [x] 4.4 `run_tui` 加 PageUp/PageDown 键 → 滚动(手动冒烟)

## 5. spinner 动画(强制 TDD + insta)

- [x] 5.1 【红】写 `advance_spinner` 测试:连调 N 次,`spinner_frame` 循环 `0→…→N-1→0`;确认失败
- [x] 5.2 【绿】`AppState.spinner_frame` + `advance_spinner`(纯,design D4);`render` 对 running 工具卡 / `CallingModel` / `ExecutingTool` 取 `FRAMES[spinner_frame]`(braille,ASCII fallback),静态态用静态 glyph
- [x] 5.3 【insta】带色快照:固定 `spinner_frame` 的 running 工具卡 / busy phase（确定帧）
- [x] 5.4 `run_tui` 的 `select!` 加 `tokio::time::interval` tick → `advance_spinner` + 重渲(手动冒烟,时间不进 render/state)

## 6. 4 帧对眼 + 收尾(insta · 对眼停点)

- [x] 6.1 【insta · 停点】`cargo insta review`:**4 themed 帧人工对眼** —— permission(+diff) `原型截图/midnight-02` + `daylight-02`、fatal(C7)`midnight-03` + `daylight-03`(连配色;config.yaml + `设计规范/README` 关卡;+ cut2b-a welcome×2 = 6 帧保真齐)。**贴 4 帧渲染给用户审**
- [x] 6.2 收尾:`cargo build`、`cargo test` 默认全绿(diff/滚动/spinner red-green + 带色 insta,无终端 / 不触网)、`cargo fmt`;自检:`tui-shell` ADDED(C6 diff / C7 / 滚动 / spinner)requirements 全有落点(red-green / insta / 对眼 已分类);偏离已标注(args-diff 非 contextual、spinner braille→ASCII、滚动键 PageUp/Down、C7 title 通用);**零新依赖 / 不碰 config·agent-loop·channel**
