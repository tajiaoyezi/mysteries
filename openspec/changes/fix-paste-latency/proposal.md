# fix-paste-latency

## Why

真机实测:610 行粘贴从 Ctrl+V 到折叠 label 出现约 **5 秒**,期间 UI 冻结无反馈。瓶颈已定位在**供给端**:Windows Terminal/ConPTY 把大粘贴切成 ~32 字符的小片、毫秒级间隔逐片投递(610 行 ≈ 数万个按键记录),而 `drain_event_batch` 的合批设计必须阻塞收完整段才能折叠——消费端(`poll(ZERO)` 抽干 + 30ms grace 桥接)已经贴地,调参无收益。耗时随粘贴量线性涨:31 行 ≈ 0.3s 无感,610 行即 5s 假死。

同类横向:正解机制 bracketed paste(整段一个 `Event::Paste`)在本栈 Windows 不可用(crossterm #737/#962,log 38 真机探针证实);同栈的 Codex CLI 用同构 burst 状态机、同样未解延迟;Claude Code 原生 Windows 粘贴正处截断/挂起 bug 多发期。事件流这条路没有更快的走法。

## What Changes

**剪贴板校准快路径**:事件流只当「发生了粘贴」的信号,内容直接取剪贴板原文——

1. **即时折叠**:`drain` 抽干首片后,批像粘贴(≥ `PASTE_COALESCE_MIN_EVENTS` 事件、全**可重建键**(`Char`/裸 `Enter`/`Tab`)、重建文本 ≥ `PASTE_FAST_MIN_MATCH_CHARS`(=8)字符——首批不足 8 字符时先做 ≤5 轮 grace **预试凑批**再恰一次尝试,防首批过小令快路径永不咬合)且无模态时,经 `Clipboard::get_text`(trait 新增方法,惰性注入,arboard 已在依赖)读剪贴板;换行归一(`\r\n`/`\r`→`\n`)后以**换行 run 感知 matcher** 做前缀匹配(对 CRLF 单/双 `Enter` 投递形态均鲁棒),命中且满足折叠阈值(行 ≥15 或字符 ≥500)→ 立即以归一化原文 `insert_paste_fold`,体感 <0.1s;行数/字符数以原文为真值。
2. **尾流校准丢弃**:折叠已完成,后续仍在到达的事件尾流(约数秒)由同一 matcher **按期望内容逐事件校准**——匹配即丢弃、**失配即转发**(人类输入天然失配:`Esc`/`Ctrl+C`/模态应答/粘贴后打字全部直通既有处理)、内容耗尽即精确清态(不依赖时钟,慢帧免疫);连续失配 ≥16 判模型失效即中止,key 静默 2s 仅作兜底。活动行右侧显示「⋯ 接收粘贴」轻提示(复用 copy_hint 渲染位,copy_hint 优先)。
3. **回退与兜底提示**:任一门槛不满足(小段粘贴、剪贴板不可读/为空/失配、模态中)→ 走**现行慢路径**(合批 + `fold_candidate`),除一次剪贴板惰性读取与一帧「⋯ 接收粘贴」提示外行为一致——5s 从假死变有反馈(即原「小修甲」)。
4. **drain 拆分**:`drain_event_batch` 拆为 `poll(ZERO)` 抽干与 grace 桥接两段,快路径判定插在中间;lone-enter 10ms 续读、`EVENT_BATCH_CAP` 双路封顶、非 Key 即收批等既有语义逐条保持。

## Impact

- Affected specs:`tui-shell`——ADDED「剪贴板校准粘贴快路径」;MODIFIED「粘贴突发合并输入」(合并机制枚举句、Tab Non-Goal 交叉引用、快路径拦截点);MODIFIED「粘贴折叠占位符」(触发段加快路径来源一句,余逐字保留)
- Affected code:`src/tui/clipboard.rs`(trait `get_text` + 两处 mock)、`src/tui/input_batch.rs`(归一/`PasteTailMatcher`/重建抽取/快路径判定纯函数)、`src/tui/app.rs`(`paste_tail` 瞬态 + 提示)、`src/tui/render.rs`(提示渲染,复用 copy_hint 位)、`src/tui/mod.rs`(drain 拆分 + 快路径分支 + 丢弃态接线 + 事件日志 disposition 标记 + 新常量)
- 纯逻辑(归一/matcher/判定/终止三层)强制 TDD;drain/loop 接线为 IO 胶水,以既有纯测零回归 + 真机清单验收
- 不新增依赖;不改 `terminal.rs`;快路径未命中时除一次剪贴板惰性读取与一帧接收提示外行为与现状一致
