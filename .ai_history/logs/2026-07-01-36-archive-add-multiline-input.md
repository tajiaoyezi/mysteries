# 2026-07-01 · 36 · archive add-multiline-input

## 决策
- 输入框加「手动多行编辑」,粘贴防狂发拆到后续 change B | 选:纯编辑内核(文本缓冲+光标+换行键+多行渲染) | 弃:本 change 内并做粘贴突发检测(需把事件循环改批量 drain 才能避免 render 耗时污染时序判定,且属启发式,独立隔离更稳) | 主导:讨论收敛 | 依据:code(crossterm WindowsEventSource 不产 Event::Paste)+ 真机 debug log
- 换行键用终端原生 modifier,不引 kitty | 选:Ctrl+Enter=Enter+CONTROL / Shift+Enter=Enter+SHIFT / Ctrl+J=Char('j')+CONTROL(WT 原生送达) | 弃:kitty PushKeyboardEnhancementFlags(Windows execute! 返 Err → TUI 起不来)、bracketed paste Event::Paste(Windows crossterm 从不产) | 主导:用户真机 debug log 实测 | 依据:code/tests。terminal.rs 不改
- 输入模型重构为纯逻辑 reducer(input_buffer:text+cursor 字节位,历史/draft 并入),无 InsertStr | 主导:讨论收敛 | 依据:TDD 红绿
- Home/End 改绑:裸=行内光标(不滚 transcript、不清选区),Ctrl+Home/End=transcript 顶/底 | 依据:tests
- 多行渲染动态框高 cap=clamp(H-16-gap-perm,1,limit)+ 软换行 logical→visual;满宽边界空续行改 cursor-aware(仅光标落该边界才补,消歧且不虚增框高)| 依据:对抗复核 finding 修复
- 提示符 mysteries ▸ → Claude 风「> 」+ 续行 2 空格悬挂对齐;layout 在缩减宽度排版、gutter 交 render(白修长首行溢出边界)| 主导:用户(6.2 真机反馈) | 依据:snapshot

## 变更
- 新增 src/tui/{input_buffer,input_layout,width}.rs;改 app.rs(on_key 换行/光标/命令单行门/模态路由 + 读写点迁移)、mod.rs(scroll_action_for_key 改绑)、render.rs(多行渲染 + 动态框高 + 「> 」gutter)
- spec:tui-shell 加 ADDED「多行输入编辑」+ 7 MODIFIED(输入历史↑↓ / transcript 滚动 / 键盘滚动 / 跳到底部 / 鼠标滚轮 / 鼠标拖选 / 文本排版)
- 4 维度对抗复核(reducer/布局/on_key/spec)确认并修 2 真缺陷:① history_up 空缓冲不刷 draft → 已删文本复活;② 非末满宽逻辑行幽灵空行。驳回 1(completion 下 Ctrl+Enter 走补全 = spec 认可取舍)

## 待决
- change B:粘贴防狂发(事件循环批量 drain + burst 判定 + pending 态吞突发 Enter),建在本 change 文本缓冲之上
- 已知取舍:超宽软换行行内不做逐显示行 Up/Down(按逻辑行);无 goal-column

## 引用
- OpenSpec change:add-multiline-input → archive/2026-07-01-add-multiline-input
- 跨越 session:本会话(fullscreen-mouse-select-copy 归档之后)
