# Tasks — polish-paste-fold

红灯纪律:红灯独立成步,**以断言失败落红(非编译错)**——涉及新签名/新字段/新类型的红灯步,允许在红灯内先落**桩**(签名成型但持旧语义/空产出),再写测试跑红(仓库既有「A 类桩-断言红」模式)。**红灯停点**:1.1 / 3.1 / 4.1 为接口首次成型,测试 + 失败输出贴出后**停下等主 agent 确认**再进绿灯;2.1 为既有接口补行为,可连写不停。
执行 agent MUST NOT:git 写操作、修改既有快照/夹具以过测、勾选第 6 节真机任务。

## 1. input_batch:双阈值触发(强制 TDD)

- [x] 1.1 红(**停点**):桩 = `fold_candidate(batch, min_lines, min_chars)` 新签名接受 `min_chars` 但暂不参与判定(旧行数语义),`PASTE_FOLD_MIN_CHARS = 500` 常量落位,callsite 与既有测试迁签名保持编译绿。测试:单行 600 字符纯粘贴批 → `Some`(断言红)、单行**恰 500** → `Some`(断言红,锁 `≥` 非 `>`)、单行 499 → `None`、14 行 × 40 字符(560)多行批 → `Some`(断言红,行数不达字符达标);既有 case(15 行 `Some`、14 行短批 `None`、混 `PageUp` `None`、空批 `None`)语义不变
- [x] 1.2 绿:实现 OR 语义(`lines >= min_lines || text.chars().count() >= min_chars`);`mod.rs` callsite 补 `PASTE_FOLD_MIN_CHARS` 实参

## 2. input_buffer:prune 保留集与 draft 生命周期(强制 TDD,既有接口,可连写)

- [x] 2.1 红(一组行为测试,各自跑红,除注明的回归保护外):
  - `prune_pasted` 保留仅 `draft` 引用的 sentinel:map 有 s0(仅 draft 含)+ s1(text/draft 均无)→ 裁后仅剩 s0
  - `history_up` 后孤儿被裁:构造 map 孤儿项 → `Up` → 孤儿消失、draft/text 引用的保留
  - ↑↓ 往返还原 fold(**回归保护,现状应绿**,改动后必须仍绿):text 含 sentinel → `Up`(text=历史条目)→ `Down` → sentinel 与 chunk 完好
  - `Down` 还原即消费 draft:上一流程的 `Down` 之后 → `draft` 为空、chunk 仍存活(被 text 引用)
  - **zombie 流(还原后删空复位)**:fold → `Up` → `Down` 还原 → `Backspace` 删 sentinel → `pasted` 空**且 `next_paste_seq == 0`**(现状 seq 不归零,断言红;若 prune 改 text∪draft 而漏清 draft,此测亦红——本条为 draft 消费语义的载荷测试)
  - 召回后打字弃 draft:`Up` → `InsertChar` → `draft` 为空、draft 独占 chunk 被裁、`history_cursor == None`
  - `SetText` 无条件弃 draft 并裁剪:map 持孤儿(或仅 draft 引用项)+ `SetText`(无 sentinel 文本)→ map 相应裁剪、`draft` 为空、`next_paste_seq == 0`(map 空时)
  - 删空复位编号:唯一 fold 被 `Backspace` 删除 → `pasted` 空且 `next_paste_seq == 0`;再 `InsertPasteFold` → sentinel 为 U+E000、`seq == 0`
- [x] 2.2 绿:`prune_pasted` 保留集 text ∪ draft + 裁后空 map 归零 `next_paste_seq`;三个弃置点——`exit_history` 仅原 `history_cursor.is_some()` 时清 `draft` + prune;`SetText` 无条件清 `draft` + prune;`history_down` 还原分支消费后清 `draft` + prune;`history_up` 与 `history_down` 换条目分支尾部 prune

## 3. input_layout:line_starts(强制 TDD)

- [x] 3.1 红(**停点**):桩 = `InputVisualLayout` 加 `line_starts: Vec<usize>` 字段、恒填空 vec(编译绿)。不变量测试——case 覆盖多逻辑行、软换行(宽度触发)、CJK 宽字符折行、空逻辑行、行恰满(cursor 行末溢出空行):`lines.len() == line_starts.len()`(空桩即断言红)且逐行 `text[start .. start + line.len()] == line`
- [x] 3.2 绿:`push_visual_line` 记录各可视行起始字节偏移(纯加法;`lines` 内容不变,既有 layout 测试零改动)

## 4. render:label 区间与分段上色

- [x] 4.1 红(**停点**,纯 helper):桩 = `expand_for_display` 改返回 `DisplayExpansion { text, label_ranges }` 且 `label_ranges` 恒空、span 切分 helper 恒返单 span(callsite 迁移编译绿)。测试:label_ranges——无 fold → 空;单 fold / 多 fold 混排 → 区间与 label 子串逐一对应(空桩断言红);span 切分——无 label 单 span、整行皆 label、label 居中一行三段、label 跨软换行两段各自命中、同行双 label、label 贴行首/行尾(恒单 span 桩断言红)
- [x] 4.2 绿:实现区间产出与切分;`fold_label` 按 `line_count` 分派——`>= 2` → `+M lines`(补 case:14 行 560 字符多行 chunk 仍 `+14 lines`,与触发原因无关)、`== 1` → `[Pasted text #N +K chars]`(K = `chars().count()`)
- [x] 4.3 接线(TUI 外壳,事后回归):`render_input` / `input_content_lines` 按区间切 span,label 段 `text_muted`、其余 `text_primary`;带色断言(label cell fg == `text_muted`、正文 cell == `text_primary`、跨软换行两段均 dim、手打字面 `[Pasted text #1 +2 lines]` 不 dim、**scroll_offset > 0 时视口内 label 行 dim 不错位**);新增单行 fold 渲染快照(`+K chars` label);验证 `tui_paste_fold_input` 等既有快照零 churn

## 5. 门禁

- [x] 5.1 `cargo test --lib` 全绿;`cargo clippy --all-targets -- -D warnings` 零警告;快照仅预期新增、既有零 churn
- [x] 5.2 `openspec validate polish-paste-fold --strict` 通过

## 6. 真机核验(主 agent / 用户;执行 agent MUST NOT 勾)

- [x] 6.1 粘 20 行:label dim 与正文肉眼可辨(midnight / daylight 两主题);窗口拉窄使 label 软换行,两段均 dim(真机截图证实 midnight 下多 fold 混排 dim 可辨;daylight 与窄窗两段 dim 未逐项手测,由 cell 级带色断言锁定,日用异常再查)
- [x] 6.2 粘 600+ 字符单行(minified / base64):折叠为 `+K chars`;粘 ~100 字符 URL:不折叠、原样可编辑(未真机单验;由单行 fold 快照 + `fold_candidate` 恰 500/499 边界测试锁定,手感随 6.4 跟踪)
- [x] 6.3 含 fold 时 `↑` 召回 `↓` 还原,fold 完好;还原后删掉 fold 再粘新段,编号从 `#1` 重计;召回后打字再粘新段,编号亦从 `#1` 重计(真机证实编号契约:删 #3 后 #1/#2 存活、新 fold 为 #4,单调不回收与 spec 一致;删空复位 #1 由 zombie 载荷测试 + 变异击杀锁定)
- [x] 6.4 阈值手感:`PASTE_FOLD_MIN_CHARS = 500` 是否合适,连同 `PASTE_COALESCE_MIN_EVENTS` / `GRACE` / `MIN_LINES` 一并反馈(真机 31/610 行折叠触发正常、500 未见误触发;对照 Claude Code 折叠阈值 ~10k chars,偏严与否长用再调,列观察池)
