# 2026-07-01 · 37 · archive guard-paste-burst-submit

## 决策
- 批量 drain 用同步 event::poll(ZERO)+read 抽干成有界 batch、整批单次 draw | 选:同步 poll/read(与 EventStream 共用 internal reader、不碰 waker) | 弃:EventStream::next().now_or_never()(noop-waker 毒化 crossterm wake-task 注册 → 输入被 120ms spinner 心跳量化成卡顿,对 crossterm 0.28.1 源码验实) | 主导:讨论收敛 | 依据:code
- 突发→换行判定纯逻辑:先滤 Release,按 Press-only 文本内容键数 n(Char 非纯 Ctrl+char + 裸 Enter),n>=2 裸 Enter→Newline、n==1→Submit | 弃:不滤 Release(Windows Press+Release 双发 → n 翻倍、提交永久失效)、把守卫消费键(PageUp 等)计入 n | 依据:tests(input_batch 10 测)
- CAP=1<<20 纯防御性上界 | 弃:小值(如 4096 → ~50 行粘贴被切、残余尾 Enter 单独成批 n==1 误判 Submit 自动提交) | 依据:design D1
- 硬模态逐键按当时活跃态分治(非整批截断) | 选:pending 首键应答后 break 丢余键、picker 每键透传 handle_models_picker_key | 弃:整批截断到首键(picker 是打字过滤多键面,截断会丢 "gpt"→"g") | 主导:round-2 对抗复核修正 | 依据:tests(④)
- InsertStr(String) 合并连续 Char:一次 clone O(n) | 弃:逐字符 InsertChar(reducer 每 action clone 含全 history → 大粘贴 O(n²) 卡顿);推翻 add-multiline-input 当时"不引 InsertStr" | 依据:design D5 + tests
- 不引 bracketed paste | 弃:EnableBracketedPaste/Event::Paste(Windows crossterm 从不产)。terminal.rs 不改 | 依据:code
- 实现方法论:红灯停点(新接口 input_batch 贴红输出停等确认)、多 reviewer 对抗核实、mutation test 证伪假绿修复 | 主导:用户(编排+审查) | 依据:本会话

## 变更
- 新增 src/tui/input_batch.rs(press_key_events 滤 Release / classify_key_batch / KeyIntent,纯逻辑 TDD);input_buffer.rs 加 InsertStr(String) reducer 动作;app.rs 加 insert_newline_and_refresh() + apply_batch_input_key() 可测分治 + flush_merged_input_chars() + 5 app 层单测;mod.rs 主循环改 drain_event_batch(poll ZERO 抽干)+ process_event_batch(前置守卫逐键 + intent 对齐 + 逐键分治),循环底仍单次 draw
- spec:tui-shell ADDED「粘贴突发合并输入」9 Scenario + MODIFIED「多行输入编辑」(动作集加 InsertStr;裸 Enter 改附条件:孤立才提交、突发批作 InsertNewline)
- review:3 维对抗复核确认实现无功能 bug,揪出并修 2 假绿测试(⑤ picker 守卫用 [Enter,Enter] 删守卫仍绿→加断言 input=="";② completion 靠 flush 关闭→改批 [Enter,Enter] 隔离 insert_newline refresh),各经 mutation test 证伪;补 1 batch 级孤立 Enter 提交护栏。420 test 全绿、clippy 零警告、快照零 churn

## 待决
- Non-Goal(D8 启发式固有上限,需真实到达时序或 bracketed paste):慢/跨周期粘贴、大 transcript 慢渲染下打字凑批、粘贴末尾换行不自动提交、粘贴含 Tab 丢失、模态关闭后同批尾 Enter 丢弃
- spec 措辞:modal_closed_in_batch 实现丢弃"该批后续所有裸 Enter",比 spec「尾 Enter」宽(仅极窄场景可达,属 D8 已记 Non-Goal),可精确化
- 复制成功轻提示(Notice「已复制 N 字」,来自 log 35)仍未做

## 引用
- OpenSpec change:guard-paste-burst-submit → archive/2026-07-01-guard-paste-burst-submit
- 前置:log 36(add-multiline-input,本 change 建其文本缓冲之上)
- 跨越 session:本会话(add-multiline-input 归档之后)
