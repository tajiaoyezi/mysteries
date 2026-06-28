# 2026-06-28 · 22 · archive tui-activity-status

## 决策
- 工作状态展示 split:动态工作状态(spinner+phase+esc 中断+token 速率)移到输入框上方活动状态行(恒占 1 行防跳动),底部状态行改 meta-only | 选:Q1 布局序 L1(活动行/input/底部 meta) | 弃:L2、全移上、底部去 meta | 主导:用户(状态移上)+ 主 agent 拍 Q1/Q2/Q4/Q6 | 依据:design ① / 设计规范 02 C10 / 参照 claude code·cursor
- token 速率可达程度(诚实):实时流式 t/s 不可得(usage 仅 complete 后回传、on_text 无 token、拒绝 tokenizer)→ ↓N tok 累计(真实)+ X t/s 完成后真实刷新 | 主导:sub-agent 调研 + 主 agent 复核 | 依据:design ③ / spec
- Q5 流式近似(用户要观感):record_streaming_chars 字符估算近似 t/s(chars/4 粗估,标 ~),完成后 on_usage 真实校正去 ~ | 选:approx(A 完成真实 + B 流式近似) | 弃:纯 A(无动感) | 主导:用户拍 Q5=approx | 依据:design ③ A+B / 测试4
- 速率计时不入 AppState:record_usage/record_streaming_chars/estimate 纯函数(elapsed 入参),Instant 在 run_tui IO task | 选:Q6 UI 侧测 CallingModel→Usage/TextDelta 间隔 | 弃:run_observed per-call(observer 带 Duration) | 主导:主 agent | 依据:design ④ / 守 spinner「时间不入 AppState」契约
- agent-loop on_usage:default no-op + run_observed 每轮 complete 后 Some 上送;run 仍 no-op observer 逐字节一致 | 主导:主 agent | 依据:design ⑤ / spec(既有 exact-sequence 不破)
- 其余:Q2 活动行恒占 1 行 + idle 显上轮 token 摘要;Q3「esc 中断」(waiting 无 esc,归权限框);Q4 仅既有 4 phase;Q7 ↓N tok = 本轮累计 output

## 变更
- agent/mod.rs:AgentObserver 加 on_usage default no-op + run_observed 上送 usage
- tui/app.rs:record_usage(真实累计+速率,清~)/ record_streaming_chars(近似,标~)/ estimate_streaming_rate_tps / estimate_tokens_from_chars(chars/4)/ idle 摘要 / 重置;tui/channel.rs:AgentEvent::Usage + ChannelObserver forward;tui/mod.rs:apply_ui_event 接线(Instant 在 IO task 测间隔)
- tui/render.rs:layout split(活动行/input/meta)+ render_activity + render_status meta-only
- spec:agent-loop MODIFY(on_usage)+ tui-shell MODIFY(C10 phase 移活动行 / meta-only)ADD(活动状态行 / token 用量速率呈现)
- 迁移 21 快照 + 新增 tui_activity_token_rates;验证:cargo test 全 target(230 lib + 1 e2e)、clippy --all-targets 零警告、fmt 净、validate 过
- 流程:propose 一次过;3 红灯停点(①on_usage ②record_usage ③流式近似)全遵守,①从编译错纠正为运行时(脚手架 trait default no-op);终审补 token 速率渲染快照(tasks 3.4 缺口)

## 待决
- TUI 真机冒烟(tasks 5.3):流式 ~t/s → usage 校正去 ~、状态栏移上、布局不跳动,留用户
- chars/4 对 CJK 偏差大(中文低估),标 ~ + 完成校正接受(观感优先,用户拍 approx)
- 速率含 channel 延迟(UI 侧 elapsed,ms 级可忽略)
- archive 时 tasks 勾选 1/21(代码+验证均完成,checkbox 未逐个回勾,实际进度见本记录「变更」)
- 承前未动:git 身份 wanglei30 临时 + leafiellune purge;summary 期间无 on_status(20)

## 引用
- change:tui-activity-status(design ①–⑤ rationale;archive 路径 changes/archive/2026-06-28-tui-activity-status)
- 前置:tui-ux-and-cli-auth(21);7 OQ 拍板:Q1 L1 / Q2 恒占1行+idle摘要 / Q3 esc中断 / Q4 仅4phase / Q5 approx / Q6 UI侧 / Q7 本轮output
- session:用户要工作状态移输入框上方+多变化 → propose(token 速率可达程度调研)→ 7 OQ 拍板(Q5 approx 用户要观感)→ 3 红灯停点(①编译错纠正运行时)→ 终审补 token 渲染快照
- memory:review-read-impl-not-just-green-tests(延续:逐行读 record/estimate + token_rate_spans 渲染码);red-light-runtime-not-compile-error(红灯①纠正)
