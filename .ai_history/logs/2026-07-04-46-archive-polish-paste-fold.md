# 2026-07-04 · 46 · archive-polish-paste-fold

## 决策

- **label dim = 区间法**:`expand_for_display` 产出 label 字节区间 + `InputVisualLayout` 纯加法暴露 `line_starts`(不变量 `lines.len()==line_starts.len()` ∧ 逐行为 display 连续切片),渲染按可视行区间与 label 区间相交切 span、label 段 `text_muted` | 弃:按 label 文本模式匹配上色(跨软换行分段后模式失配——恰是最需要 dim 的窄视口场景;手打同款字面文本误染)、sentinel-aware layout(侵入已测折行,违纯加法) | 主导:讨论收敛(落地 v1 D5 预留方向) | 依据:code + cell 级带色断言(含 scroll_offset>0 错位杀)
- **单行超长折叠**:触发改 `行数 ≥ 15 || 字符数 ≥ PASTE_FOLD_MIN_CHARS(=500)`;label 按 chunk 的 `line_count` 分派**与触发原因无关**(≥2 → `+M lines`,==1 → `+K chars`,K 按 `chars().count()` 与「已复制 N 字」同口径) | 弃:`PastedChunk` 缓存 `char_count`(每帧 ~4×O(len) 现算可忽略,无 profile 不预优化) | 依据:tests(恰 500 锁 `≥`、14×40 多行字符达标仍 `+14 lines`)
- **prune 保留集 text ∪ draft + 三弃置点**:`exit_history` 门控清(原 `history_cursor.is_some()`,热路径零开销)、`SetText` 无条件清、`history_down` 还原分支**消费即清**;`pasted` 清空即归零 `next_paste_seq` | 弃:text-only 保留集(召回途中杀 draft 引用 chunk,`↓` 还原后 sentinel 裸奔丢数据)、全动作统一 prune(为不存在的孤儿扫热路径) | 主导:对抗审查坐实「还原后 stale draft → zombie chunk → 编号不归零」可达序列后定案(还原消费为审查修入,非原稿) | 依据:zombie 载荷测试 + 变异
- **流程**:propose 经 3 路对抗审查(代码事实/契约一致性/边界攻击):0 HIGH、2 MED 修入(还原消费语义;红灯步改「桩-断言红」防编译错落红)、16 攻击面扛住(dim 数学、两份 expand 一致性、SetText 入口盘点、复制路径);dispatch 3 停点(1.1/3.1/4.1)**首次全被执行 agent 遵守**,主 agent 每停点自跑复现 + 逐行读码;收尾 6 组变异全杀(去字符阈值/prune 退 text-only/还原不消费/切分无视区间/行号改相对/label 不分派)
- **真机**:midnight 多 fold 混排 dim 可辨、编号契约(删 #3 后 #4、不回收)截图证实;**大粘贴 ~5s 为独立固有问题**(ConPTY 分片限速投递,消费端已贴地;非本 change 回归)→ 另立 change

## 变更

- `input_batch.rs`:`fold_candidate(batch, min_lines, min_chars)` OR 语义 + `PASTE_FOLD_MIN_CHARS=500`
- `input_buffer.rs`:`prune_pasted` text∪draft + 空 map 归零 seq;三弃置点清 draft;`history_up/down` 尾部 prune
- `input_layout.rs`:`line_starts`(空行=行首、wrap 段=段首字节、行满溢出空行=行末)
- `render.rs`:`DisplayExpansion { text, label_ranges }`、`input_content_spans` 相交切分、`fold_label` 分派;带色断言 ×4 + 单行 fold 快照
- spec:tui-shell MODIFIED「粘贴折叠占位符」(双阈值/label 分派/dim 区间契约/draft 生命周期/编号复位;Non-Goals 摘除单行超长、PUA 撞车含归零扩窗口径)
- 测试 519 → 548;既有快照零 churn

## 待决

- **大粘贴 5s 提速**(本次收口时用户已知、择期):方案 A 剪贴板校准(检测突发→arboard 读原文即时折叠,尾流比对丢弃,不匹配回退现行;推荐,顺带以原文为行数真值)vs B 流式渐进 vs 小修甲(仅加「接收中」提示帧)。同类对照:bracketed paste 本栈 Windows 不可用(crossterm #737/#962 + log 38 探针);Codex 同栈同为 burst 状态机、未解延迟;Claude Code 原生 Windows 粘贴正处截断/挂起 bug 多发期
- CRLF「双计」假说未证实(探针日志无粘贴突发样本);方案 A 下自然消解
- daylight 主题 dim 对比、单行 `+K chars`、窄窗两段 dim:真机未逐项验,由带色断言/快照锁定
- 阈值 500 偏严与否:对照 Claude Code ~10k chars,长用再调
- 字面 PUA 撞车(归零后 sentinel 复用扩窗)维持接受边界

## 引用

- OpenSpec change:`polish-paste-fold`(feat b0df656)→ archive/2026-07-04-polish-paste-fold
- 相关 log:[[2026-07-03-40-archive-add-paste-fold]](v1 与遗留清单出处,本件收其三尾)、[[2026-07-04-44-archive-add-diff-highlight]](带色断言模式沿用)
- 跨越 session:本会话(propose → 3 路对抗审查 → 3 轮停点 dispatch → 主 agent 逐停复现 + 变异 → 真机截图核验 → 收口;粘贴提速与 1.1 工程对齐为后续件)
