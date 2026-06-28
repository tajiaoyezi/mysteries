# 2026-06-28 · 15 · archive claude-code-style-loop

## 决策
- claude-code 式循环:跑到自然结束 + 高位安全网(DEFAULT 8→50)+ 触顶强制收尾(禁用 tools 再调一次逼文字;仍空才 MaxIterations;该次 provider Err→Provider)| 选:强制收尾 | 弃:直接 Err(反模式)、静默截断 | 主导:用户「直接做成 claude code 形式」+ 调研(Codex~60 / OpenCode 触顶总结)| 依据:design ① / code
- 运行中可中断:专用 interrupt channel(不复用 input_rx)+ tokio::select! drop 协作取消;Esc 三态(pending=拒绝 / 运行中=Interrupt / 就绪=退出,仅 Press)| 弃:input_rx 选(误吞 Prompt)、AbortHandle(过重)| 主导:主 agent 红灯②打回(测试原走 input_rx → 纠正为专用通道)| 依据:design ② / 测试「不消费排队 Prompt」(calls==2)
- 单一时间线:TranscriptBlock::Tool 单 Vec(Started push / Finished 按 id 回填 / 无匹配安全忽略),删 tool_cards 字段;最终回答钉底即见;msgs 只计 User/Assistant | 弃:双 Vec 时间戳归并、内联 Assistant 块 | 主导:主 agent 红灯③打回(断言原引用将删的 tool_cards → 纠正为正向断言 Tool 块)| 依据:design ③
- 据实纳入 D(工作树既有 8 项 TUI 优化):身份约束、按键去重、换行+悬挂缩进、欢迎居中、emoji/零宽宽度、输入光标、◆ marker(顺带修了 HEAD 的 "m " marker bug)、行级+鼠标滚动 | 选:Option A 据实全纳入,随 A/B/C 一并提交 | 主导:用户拍板 | 依据:git diff 核实 + 主 agent review(clippy 零警告、修了真 bug)

## 变更
- agent/mod.rs(强制收尾)、config/mod.rs(50)、tui/{app,channel,mod,render,terminal}.rs(中断+时间线+D)、snapshots(5 改 3 新)
- 验证:cargo test 全 target 166 passed / 2 ignored + e2e 1;clippy --all-targets 零警告;fmt 净;validate --strict 过;src 无 tool_cards 残留
- 流程:3 红灯停点(强制收尾 / 中断 / TranscriptBlock::Tool)各打回纠正后通过;②③ 由主 agent 纠正机制错误(input_rx→专用通道、tool_cards 断言→正向)
- archive 时 tasks 21/24:6.1/6.2(cargo / validate)由主 agent 终审完成未回勾、6.3(TUI 手动冒烟)待用户实跑

## 待决
- 强制收尾那次 complete 前未 emit CallingModel:触底(50)瞬间状态栏短暂停在「执行工具…」(罕见路径,TurnComplete 自纠)
- 鼠标滚轮在 Windows Terminal 实测无响应未根治(本 change 仅纳入既有滚轮管线);连同 ↑↓/Home/End、工具折叠+ctrl+o 留 improve-tui-interaction
- 写工具 execute 被中断 drop 可能留半写文件(Non-Goal,v1.x 接受)
- git 身份仍临时 wanglei30;旧 leafiellune 在 refs/original+reflog 待 purge
- target-codex/ 残留构建目录待清理 / gitignore

## 引用
- change:claude-code-style-loop(design ①–④ 全量 rationale;archive 路径 changes/archive/2026-06-28-claude-code-style-loop)
- 调研:Claude Code agent-loop docs / Codex / OpenCode(触顶总结范式)
- 前置:finish-1-0(14);后续:improve-tui-interaction
- session:主 agent 编排——propose 审查 + 3 红灯停点(②③ 纠正)+ D 来历核实(用户另派 agent 的 TUI 优化,据实纳入)+ 全量终审
