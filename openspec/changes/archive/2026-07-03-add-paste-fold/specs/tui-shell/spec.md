## ADDED Requirements

### Requirement: 粘贴折叠占位符(大段粘贴折叠为原子 token)

TUI SHALL 在输入缓冲层把**大段粘贴**折叠为一个原子占位符 token:仅当输入来自粘贴(批级识别)且逻辑行数 `≥ PASTE_FOLD_MIN_LINES`(=15)时 MUST 折叠,手打多行与小段粘贴 MUST 照原样逐字符插入。占位符在输入框渲为一行 `[Pasted text #N +M lines]`,提交时 MUST 原位展开为完整文本。

**触发(批级重建)**:纯函数 `fold_candidate(batch, threshold) -> Option<String>` SHALL **仅当批的 press 键(`press_key_events`)逐键全为文本内容键**(`Char` 非纯 Ctrl + 裸 `Enter`,整批本质是一段纯粘贴)时,重建粘贴文本(`Char`→字符、裸 `Enter`→`\n`)并判逻辑行数(`split('\n').count()`,= 裸 Enter 数 + 1)`≥ threshold` 返回 `Some(文本)`;只要出现任一非文本内容键(如 `PageUp`)或行数不达阈值,MUST 返回 `None`。`process_event_batch`(非 `run_tui` 主循环,其顶部持整批 `Vec<Event>`)SHALL 在无 `pending_permission`、无 `models_picker` 且 `fold_candidate` 命中时调 `insert_paste_fold`(`pub(crate)` 入口,封装私有 `apply_input_action`+`refresh_command_completion`)并消费整批,否则走既有逐键路径;paste-guard 跨批续读在上游 `drain_event_batch`,不受影响。

**存储(原子单字符 + 旁挂映射)**:`InputBufferState` SHALL 持 `pasted: BTreeMap<char, PastedChunk>`(`PastedChunk { seq, text, line_count }`)与 `next_paste_seq: u32`。`InsertPasteFold(s)` 动作 MUST 在 `cursor` 处插入一个私有区单字符 sentinel(`char::from_u32(0xE000 + seq)`)、记录 `pasted[sentinel] = { seq, text: s, line_count: s.split('\n').count() }`、`next_paste_seq += 1`。sentinel 为**单个 `char`**,故现有基于 char 边界的光标移动与 `Backspace`/`Delete` 逻辑 MUST NOT 改动即把占位符当**一个原子**(整体跨过、整体删除、光标不入内部)。

**编辑与清理**:占位符 SHALL 可与手打文字混排、可多个;`Backspace`/`Delete` 删除 sentinel 后 MUST `prune_pasted()`(`pasted.retain(|c,_| text.contains(*c))`),不留孤儿映射。显示编号 `#N = seq + 1`(seq 从 0,渲染时 +1;消息内单调、删后不回收)。

**提交(展开)**:提交时 prompt MUST 取 `expand_folds`(逐 char 把 sentinel 替回 `PastedChunk.text`)后的完整文本;命令旁路判据 MUST 计入 fold(`input().contains('\n') || input_has_fold()` 为真时不试 `parse_command`)。`PushSubmitted` MUST 以展开文本入 history,并清空 `pasted` + 置 `next_paste_seq = 0`(故 `↑` 召回显示展开文本、下一条 `#N` 从 `#1`(seq 0)起)。

**渲染**:render 端 SHALL 以 `expand_for_display`(sentinel → `[Pasted text #N +M lines]`)+ 光标偏移映射喂现有 `visual_input_layout`,换行与 `input_content_height_cap` 按 label 宽度计入;label 以可辨样式渲染(dim 为目标,跨软换行分段上色若需侵入已测 layout 则 v1 兜底为正文样式,由锁定快照为准)。空 `pasted` 时输入渲染 MUST 与折叠前一致(既有输入快照零 churn)。

**Non-Goals(v1)**:不支持 `↑` 编辑/回折已提交的粘贴;不折叠单行超长(无换行)粘贴(触发按行数);不改 `apply_batch_input_key` 既有逐键路径;不保证 `↑↓` 垂直移动跨越/邻接 fold 时光标落位列与屏幕 label 宽度对齐(reduce 按 buffer 列 sentinel=1 算,label ~26 列,v1 接受、真机勿当回归)。

#### Scenario: 大段粘贴(≥15 行)折叠为占位符 token

- **WHEN** 一批粘贴事件重建出 20 逻辑行文本,`process_event_batch` 时无模态
- **THEN** `fold_candidate` 返回 `Some(该文本)`;施 `InsertPasteFold` 后 `input_line.text` 在光标处含**一个** sentinel、`pasted` 有一项(`line_count=20`)、`next_paste_seq=1`;输入框该处渲为一行 `[Pasted text #1 +20 lines]`,不逐行撑满

#### Scenario: 小段粘贴(<15 行)与手打多行不折叠

- **WHEN** 批重建出 14 逻辑行(或手打的多行)
- **THEN** `fold_candidate` 返回 `None`;走既有逐键路径,文本逐字符/逐行进缓冲,`pasted` 为空、渲染同现状

#### Scenario: 占位符为原子——方向键整体跨过、退格整体删除

- **WHEN** 缓冲为 `a⟦sentinel⟧b`(⟦⟧ 为一个折叠占位符),光标在末尾;先按 `MoveLeft` 两次,再于 sentinel 后按 `Backspace`
- **THEN** `MoveLeft` 一次跨过 `b`、再一次整体跨过占位符(光标落 `a` 后);`Backspace` 于 sentinel 后整体删除该占位符(不进入内部)、随后 `prune_pasted` 使 `pasted` 移除该项

#### Scenario: 提交展开为完整文本、history 存展开文本

- **WHEN** 缓冲为 `看这段:⟦#1(20 行原文)⟧`(单 sentinel、无字面 `\n`),按 Enter 提交
- **THEN** prompt = `看这段:` + 20 行原文(`expand_folds`);因含 fold **不**试 `parse_command`;transcript/history 收展开全文;提交后 `pasted` 空、`next_paste_seq=0`;`↑` 召回该条显示展开文本

#### Scenario: 混排多占位符按位置展开保序

- **WHEN** 缓冲为 `⟦#1⟧ 中间 ⟦#2⟧`,两占位符各自原文 A、B
- **THEN** `expand_folds` 得 `A 中间 B`(保序、各归各位);渲染为 `[Pasted text #1 +.. lines] 中间 [Pasted text #2 +.. lines]`

#### Scenario: 折叠触发纯函数(可单测)

- **WHEN** 对 `fold_candidate(batch, 15)` 分别给:14 个裸 `Enter`(+若干 `Char`,重建 15 逻辑行、`count==15`)的纯粘贴批、13 个裸 `Enter`(14 逻辑行、`count==14`)批、含 `PageUp` 的混批、空批
- **THEN** 依次:`Some(重建文本)`(裸 `Enter` 成 `\n`、`count==15`)/ `None`(`count==14` 不达阈值)/ `None`(含非文本内容键)/ `None`;边界口径:N 逻辑行 = N−1 个裸 Enter(无尾随)、`count = 裸 Enter 数 + 1`

#### Scenario: 折叠渲染与高度核算(insta 快照)

- **WHEN** 输入框含 `前缀文字 [Pasted text #1 +20 lines] 后缀文字`,`TestBackend` 渲染;另测窄宽两宽度
- **THEN** 占位符渲为一行 label、与正文可辨;`visual_input_layout` 按 label 宽度换行、`input_content_height_cap` 不因 fold 偷 transcript 地板;空 `pasted` 布局同现状,与锁定快照一致
