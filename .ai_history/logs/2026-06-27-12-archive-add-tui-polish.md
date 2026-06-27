# 2026-06-27 · 12 · archive add-tui-polish

## 决策

- **TUI 收官(cut2b-b)**:补 §8 完整观感最后四样(C6 diff / C7 致命框 / 滚动 / spinner)+ cut2b-a 待决 n-color fix;一刀 | 依据:§8 / §9 / 设计规范 03
- **D1 C6 diff = `compute_diff(tool_name, args)` 纯函数、不读文件**:write `content`→Add、edit `old_string`→Del/`new_string`→Add、shell 空 | honest args-diff(非工作树 contextual):edit 的 `old_string` 因工具要求唯一匹配(§5.3)即真实将替换片段 | 弃读文件 contextual diff(非确定、1.0 不必要)
- **D2 C7 = 渲已有 `TranscriptBlock::Error`,无新事件**(`error.bg/border/fg` + title),不动 channel/agent-loop
- **D3 滚动 = top-anchored `scroll_offset`**:跟随底部 / PageUp·PageDown / 非底保持不强拉 / clamp;只 transcript 区;offset 逻辑 red-green
- **D4 spinner 确定性三分**:`advance_spinner`(`(frame+1)%len` 纯,red-green)/ `render` 吃固定 `spinner_frame`(insta 锁固定帧)/ tick 在 `run_tui` select!(手动)| **时间从不进 render/AppState** | FRAMES=braille + ASCII fallback
- **D5 n-fix**:权限框 `[n · 拒绝]`→`error.fg`(01「拒绝=error.fg」,cut2b-a 待决落实)
- **D6 测试分界**:`compute_diff`/`scroll_offset`/`advance_spinner` = red-green;diff/C7/滚动窗/spinner 固定帧 = 带色 insta;4 themed 帧 = 对眼;tick/终端 = 手动
- **capability**:只 MODIFIED `tui-shell`(+4 ADDED);**不碰 config/agent-loop/channel**
- **对眼停点 §6.1**:4 themed 帧(permission+diff / fatal × midnight/daylight,对 原型 02·03)
- **审查修正**:§6.1 对眼发现工具卡**多行 output 每行重复「output: 」**(cut2a 单行写法套多行、首现于此帧)→ 改直接逐行显 `text.body`(对 C5 + 原型)| 主 agent 对眼
- **honest deferral**:C7 title 因 `AgentEvent::Error(String)` 通用 → 渲「致命错误」+ message,provider 特定 title 留事件携结构后(Open Question)
- **里程碑**:TUI(§8)完整 —— 四区 + 工具卡 + 全 phase + 双主题 + C6 diff + C7 + 滚动 + spinner;welcome/permission/fatal × midnight/daylight **6 帧设计保真齐**

## 变更

- `src/tui/app.rs`(`compute_diff`/`DiffLine` + `scroll_offset`/`visible_scroll_offset`/`page_up·down` + `spinner_frame`/`advance_spinner` + SPINNER/ASCII FRAMES);`render.rs`(diff body + C7 框 + 滚动窗 + spinner 帧 + n-fix + output 渲染修);`mod.rs`(run_tui 加 interval tick + PageUp/Down 键)
- 9 快照(5 改 + 4 新:permission_daylight / fatal / fatal_daylight / transcript_scroll_window)
- 验证:`cargo test` 129 passed / 1 ignored;`clippy --all-targets` 零警告;`fmt` 通过;**零新依赖**(`Cargo` 无 diff)
- archive:`changes/add-tui-polish` → `changes/archive/2026-06-27-add-tui-polish`;`specs/tui-shell` +4 ADDED

## 待决

- **step5 收尾(1.0 仅剩此步)**:Anthropic provider 实装、内置命令(C8/C9,`/help /clear /model /status /exit` …)、流式打磨 / 超时 / 重试微调
- `ToolOutcome.exit` 字段(恢复工具卡 exit foot)→ tool-system / 收尾
- 256/16 色降级 → 1.4;运行时主题切换(config `theme` / `/theme`)、滚动行级·Home/End·输入历史、spinner tick 空闲暂停、C7 provider 特定 title(需 `AgentEvent::Error` 携结构)→ 体验 / 后续
- **§8 TUI 完整;§12 step4 完成**

## 引用

- change:`add-tui-polish`(rationale / rejected alternatives 全量见 design.md D1–D6;archive 路径 `changes/archive/2026-06-27-add-tui-polish`)
- 技术方案 §8 / §9 / §12 step4(完成)
- `设计规范/03` C6/C7、`02`、`原型截图/midnight-02·03 + daylight-02·03`(4 帧对眼)
- 前置 change:`add-tui-theme`(决策记录 11)
- session log:无专属 checkpoint —— 子 agent propose + implement(停点 §6.1 4 帧对眼);主 agent review(核 `compute_diff` 对 arg schema、spinner 确定性、scroll 模型、对眼打回 output 逐行重复、确认 n-fix)+ commit / archive
