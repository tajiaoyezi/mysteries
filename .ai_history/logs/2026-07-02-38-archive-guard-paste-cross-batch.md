# 2026-07-02 · 38 · archive guard-paste-cross-batch

## 决策
- 修"粘贴行数过多自动发送"用 P3(drain 提交前续读)| 选:drain 抽干 poll(ZERO) 后,落单裸 Enter(纯函数 would_submit_lone_enter 为真)时以 poll(GRACE=10ms) 探测终端有无紧跟【键盘】续批——有则读入同批经既有 classify 判换行、无则提交;信号取自 drain 内同步 poll,不经 draw/select!/墙钟 | 弃:P1 墙钟到达间隔(记 last_batch_end、gap<20ms 判续批)——被第一轮对抗审查以真源码否决:last_batch_end 在 draw 前更新令 gap 含整帧重绘时延、agent 流式 TextDelta 在 select! 穿插各自 draw 撑大 gap、EnableMouseCapture 的鼠标 Moved 批也刷新基线致手敲 Enter 被降级漏提交,双向失败(既误提交又漏提交);S bracketed paste / B 靠换行 modifier 本栈不可用(探针证实 crossterm 在 WT/ConPTY 无 Event::Paste、换行 modifiers=NONE)| 主导:两轮对抗审查 + 讨论收敛 | 依据:真机诊断探针 + code + crossterm issue #737/#962
- 续读只等键盘续批:poll(GRACE) 读到非 Event::Key(鼠标 Moved/Focus/Resize)即收批 | 弃:整批谓词恒真续读(would_submit 只看 press_key_events 滤后键、鼠标 Moved 被滤致谓词不翻转 → 高频 Moved 令续读不退出、drain 同步阻塞致 UI/agent 流式停摆、CAP 只在 poll(ZERO) 路径够不到)| 主导:第二轮对抗审查 finding | 依据:code
- 不做 intent 降级、续读触发抽纯函数 would_submit_lone_enter | 选:续批读入同批后复用既有 classify_key_batch(裸 Enter 因 n 变大自然从 Submit 变 Newline)| 弃:墙钟方案里"gap 近时把 Submit 降级 Newline"内联在不可测的 process_event_batch(headline 场景零覆盖)| 依据:tests(would_submit 6 测含"续批粘入 Char 后转 false")+ CLAUDE.md TDD 边界
- 为何"非 Key 即停"而非"非 Char 即停" | 粘贴中段换行的 Enter Release 会先于下一行 Char 到达,读到 Release 就停会把中段换行误判提交;故键盘事件(含 Release)一律继续续读,只有非键盘事件才停 | 依据:诊断探针(Enter Press/Release 分批到达)

## 变更
- 新增 src/tui/input_batch.rs::would_submit_lone_enter(纯逻辑 TDD,6 测:落单 Enter→true / Release 不破坏 / n≥2→false / 空批→false / 续批粘入 Char→false)
- src/tui/mod.rs:drain_event_batch 改续读循环(while poll(ZERO) 抽干 + would_submit 门控 poll(GRACE) 续读 + 非 Key 即 break + 抽干/续读两路 EVENT_BATCH_CAP);加 PASTE_CONTINUATION_GRACE=10ms 常量。process_event_batch / classify_key_batch / press_key_events / apply_batch_input_key / select! 事件循环 / terminal.rs 一行未改
- spec:tui-shell MODIFIED「粘贴突发合并输入」——叠加"提交前续读"段 + 新 3 Scenario(续读并入判换行 / would_submit 纯函数真值表 / 非键盘事件即收批)+ 收窄 D8"跨批/跨周期粘贴换行落单误提交"、原样保留 Non-Goal①(大 transcript 慢渲染下正常打字凑批)
- 附带(polish,不走 change,随本提交):src/tui/clipboard.rs 复制成功提示「已复制 N 字」(char 数非字节数,CJK 正确)

## 待决
- 大段粘贴逐行展开进输入框、跨批多次全屏重绘 → 卡顿。下一个 change:Claude 式 `[Pasted text #N +M lines]` 折叠占位符(先 brainstorm:折叠阈值 / 占位符 token 存储渲染提交展开 / 跨批聚合判粘贴边界 / Backspace 删 token 交互;本栈无 bracketed paste 只能近似聚合)
- ~~滚轮偶发"操作多行输入框"现象未复现(诊断日志证滚轮=Mouse ScrollUp/Down、非 Key,代码只滚 transcript scroll_offset、不碰输入框);待复现再查~~ **已闭案(2026-07-04,见 [[2026-07-04-45-archive-fix-shell-clobbers-mouse-capture]])**:根因 = run_shell 子进程重置 console 输入模式后,终端把滚轮降级为 ↑/↓ 方向键落入输入路径;当时诊断到 Mouse 正常是因为该会话模式尚未被冲掉
- guard 保留 Non-Goal:粘贴以落单换行收尾仍提交、续批间隔>GRACE 的极端(跨秒慢粘贴)、粘贴含 Tab、模态关闭后同批尾 Enter 丢弃

## 引用
- OpenSpec change:guard-paste-cross-batch → archive/2026-07-02-guard-paste-cross-batch
- 前置:log 37(guard-paste-burst-submit;本 change 补其 D8 跨批 Non-Goal)
- 两轮对抗审查 workflow:第一轮(5 维 · 15→6 CONFIRMED · 否决 P1 墙钟方案)、第二轮(4 维 · 验证 P3 · 余 1 minor 续读终止性,已修入"非 Key 即停")
