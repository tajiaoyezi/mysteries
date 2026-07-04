# 2026-07-04 · 48 · archive fix-paste-latency

## 决策
- 大粘贴 5s 假死走「剪贴板校准快路径」| 选:事件流只当「发生了粘贴」的信号,内容取剪贴板原文,matcher 前缀校准命中即折叠 | 弃:bracketed paste(crossterm Windows 不可用,#737 / #962,log 38 探针实证)、消费端调参(已贴地;供给端 ConPTY ~32 字符/片是物理节奏)、定时静默丢弃尾流(初版设计,模态应答 / Esc 吞键 + 慢帧自续期两处 HIGH,机制整体替换为内容校准)| 主导:讨论收敛 | 依据:两轮对抗核对 + 四轮真机迭代
- 尾流失配处置 = 失配转发 + 16 连失配进护栏态(可重建键恒弃、2s 静默兜底收场)| 弃:失配即整体清态(11.9 万事件涌普通路 → 分钟级卡死,真机二轮实证)| 依据:debug 日志取证
- 不可靠字符(astral > U+FFFF / U+FE0F / U+200D)期望侧跳过重试 | 弃:照常失配(国旗 emoji 处恒中止)| 依据:泄漏事件计数取证证实 ConPTY 对 astral 零投递
- 四轮真机迭代定案:凑批预试(首批 < 8 字符快路不咬合)→ 护栏态 + BufWriter 日志(逐行 open-close 是卡死放大器)→ 尾流纯丢弃批跳过重绘 → emoji 跳过 | 主导:用户真机反馈驱动
- 已知限制暂接受(用户拍板):尾流接收期(数秒)Enter 发送不可靠——用户 Enter 与内容换行在事件层不可区分,撞车即被吞,需待提示消失后回车 | 弃(本轮):tiny-batch 启发式区分用户 Enter(洪流片界孤 Enter 会假阳性误提交,即 [[2026-07-01-37-archive-guard-paste-burst-submit]] 修过的 bug 的复活路径)
- 附带发现:慢路径事件重建一直静默丢 emoji(ConPTY 不投递);快路径取剪贴板原文,反而顺带修复此保真问题

## 变更
- tui-shell delta:ADDED「剪贴板校准粘贴快路径」;MODIFIED「粘贴突发合并输入」「粘贴折叠占位符」
- `clipboard` trait 增 `get_text`;`input_batch` 增归一 / 双口径重建 / `PasteTailMatcher` / 快路径判定;`app` 增 `paste_tail` 瞬态;`render` 增「⋯ 接收粘贴」;`mod.rs` drain 拆分 + 接线
- 测试 548 → 585;真机证据:1085 行即时折叠零泄漏;日志 fast-paste 76 / tail-drop 24 万 / abort 0 / decline 1(no-match,自然失配不误折)

## 待决
- 尾流期 Enter 发送不可靠:若后续要解,方向是压缩尾流时长(无 debug 下时长未采集)或读取吞吐提速,不走 Enter 启发式
- 折叠阈值 500 字符(Claude Code 约 10k)—— 观察池
- 7.2 部分子项(尾流 Esc / 滚轮)、7.3 模态尾流、7.5 逐项回归未逐验,单测覆盖,入观察池

## 引用
- OpenSpec change:fix-paste-latency
- Session log:[[2026-07-02-38-archive-guard-paste-cross-batch]](bracketed paste 探针、lone-enter 续读)、[[2026-07-04-46-archive-polish-paste-fold]](折叠卫生)
- 同批先行归档:[[2026-07-04-47-archive-align-permission-spec-terms]]
