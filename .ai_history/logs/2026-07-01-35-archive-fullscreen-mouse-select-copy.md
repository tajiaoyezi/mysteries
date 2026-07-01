# 2026-07-01 · 35 · archive-fullscreen-mouse-select-copy

## 决策
- 全屏下三件全有(滚轮 + 拖选复制不按 Shift + ↑↓ 历史)走「app 自管选区 + 复制」| 选:全屏 + 捕获鼠标 + app 自绘选区/自写剪贴板(opencode 路)| 弃:内联渲染(Codex/Claude Code 路,三件终端原生免费,但布局观感被用户否)、Shift+拖选(用户拒)、仅开滚轮不捕获(alt-screen 无原生 scrollback)| 主导:用户拍板 | 依据:终端鼠标协议 + 真机反馈
- 剪贴板用 arboard | 弃:OSC52(WT 上不稳、本机不走 SSH)| 主导:用户确定
- 松开复制后保留选区高亮 | 弃:复制即清(会使「Ctrl+C 有选区复制」永不触发,与用户要的两种复制都要矛盾)| 主导:用户拍板(审查暴露 design/tasks 内部矛盾)
- Ctrl+C/Esc 优先级 = pending_permission(模态) > 选区 > 中断/退出 | 弃:选区最高优先(会凌驾阻塞式授权模态)| 主导:审查判定 + 用户采纳 | 依据:审查 finding F
- 选区取文按 cell 显示宽度跳延续格,不靠 is_empty() | 依据:审查读 ratatui 0.29 真源码坐实——宽字符延续格 symbol 是单空格 " " 非空串,朴素 is_empty 会把「你好」复制成「你 好」| 主导:审查 finding A(code 权威)
- 取帧用 CompletedFrame.buffer 非 current_buffer_mut()(swap 后指向被 reset 的空白 back buffer)| 主导:审查 finding I

## 变更
- 新增 selection.rs(纯逻辑归约,TDD)、clipboard.rs(Clipboard trait + ArboardClipboard 降级 + copy_selection,TDD)
- mod.rs 事件循环:鼠标分流 + 松开复制、Ctrl+C/Esc 四级优先级、滚轮/键盘滚动/resize 清选区、last_frame 存 CompletedFrame.buffer
- app.rs:AppState.selection + apply/clear/has;Enter//clear 清选区
- theme.rs:+selection_bg(双调色板,accent 底,钉死 #3f3455/#e0d8e4)
- render.rs:highlight_selection overlay(逐 cell 上 bg,含宽字符延续格)、selection_text(宽度跳格)
- spec tui-shell:ADDED 鼠标拖选与复制;MODIFIED 运行中可中断、鼠标滚轮滚动
- 附带 UI polish:输入提示右对齐 + 浮层态隐藏(避终端 IME 组合浮层重叠),18 张满帧快照更新

## 待决
- 多行输入:粘贴多行现逐 Enter 提交(未开 bracketed paste)、无 Ctrl+Enter 换行 —— 下一个 change
- 复制成功轻提示(Notice「已复制 N 字」)v1 未做

## 引用
- OpenSpec change: fullscreen-mouse-select-copy(archive 2026-07-01)
- 设计前跑了对抗性审查 workflow(4 维并行 + 逐条独立复核):19 条 → 13 坐实 / 6 驳回,全修入 spec 后再实现
