# tui-shell Delta

## MODIFIED Requirements

### Requirement: 粘贴折叠占位符(大段粘贴折叠为原子 token)

TUI SHALL 在输入缓冲层把**大段粘贴**折叠为一个原子占位符 token:仅当输入来自粘贴(批级识别)且满足**逻辑行数 `≥ PASTE_FOLD_MIN_LINES`(=15)或字符数 `≥ PASTE_FOLD_MIN_CHARS`(=500)之一**时 MUST 折叠,手打多行与小段粘贴 MUST 照原样逐字符插入。占位符在输入框渲为一行 label,形态**按 chunk 的 `line_count` 分派、与触发原因无关**——多行 chunk(`line_count >= 2`,含行数不达而字符达标者)为 `[Pasted text #N +M lines]`,单行 chunk(`line_count == 1`)为 `[Pasted text #N +K chars]`(K = `chars().count()` 字符数,非字节数)——以 `text_muted` 弱化样式与正文区分;提交时 MUST 原位展开为完整文本。

**触发(批级重建)**:纯函数 `fold_candidate(batch, min_lines, min_chars) -> Option<String>` SHALL **仅当批的 press 键(`press_key_events`)逐键全为文本内容键**(`Char` 非纯 Ctrl + 裸 `Enter`,整批本质是一段纯粘贴)时,重建粘贴文本(`Char`→字符、裸 `Enter`→`\n`)并当 `split('\n').count() >= min_lines` **或** `chars().count() >= min_chars` 时返回 `Some(文本)`;只要出现任一非文本内容键(如 `PageUp`)或两阈值均不达,MUST 返回 `None`。`process_event_batch`(非 `run_tui` 主循环,其顶部持整批 `Vec<Event>`)SHALL 在无 `pending_permission`、无 `models_picker` 且 `fold_candidate` 命中时调 `insert_paste_fold`(`pub(crate)` 入口,封装私有 `apply_input_action`+`refresh_command_completion`)并消费整批,否则走既有逐键路径;paste-guard 跨批续读在上游 `drain_event_batch`,不受影响。

**存储(原子单字符 + 旁挂映射)**:`InputBufferState` SHALL 持 `pasted: BTreeMap<char, PastedChunk>`(`PastedChunk { seq, text, line_count }`)与 `next_paste_seq: u32`。`InsertPasteFold(s)` 动作 MUST 在 `cursor` 处插入一个私有区单字符 sentinel(`char::from_u32(0xE000 + seq)`)、记录 `pasted[sentinel] = { seq, text: s, line_count: s.split('\n').count() }`、`next_paste_seq += 1`。sentinel 为**单个 `char`**,故现有基于 char 边界的光标移动与 `Backspace`/`Delete` 逻辑 MUST NOT 改动即把占位符当**一个原子**(整体跨过、整体删除、光标不入内部)。

**编辑与清理(孤儿裁剪)**:占位符 SHALL 可与手打文字混排、可多个。`prune_pasted()` 的保留集 SHALL 为 **`text` ∪ `draft`** 中出现的 sentinel(历史召回时 chunk 可能仅被 `draft` 引用,text-only 裁剪会使 `↓` 还原后 sentinel 失配丢数据);`Backspace`/`Delete` 删除后、`SetText`(如命令补全)与 `history_up`/`history_down` 替换 `text` 后 MUST `prune_pasted()`。使 draft 还原路径永久不可达的动作 MUST 弃 draft(清空 `draft` 后裁剪),共三处:`exit_history`(召回态打字/编辑退出,仅原 `history_cursor.is_some()` 时生效,非召回态打字零额外开销)、`SetText`(无条件)、`history_down` **还原分支**(Some→None,消费即清空:`text = draft` 后置空 draft——不清则 stale draft 令删除后的 chunk 变 zombie、编号不归零)。裁剪后 `pasted` 为空时 `next_paste_seq` MUST 归零(无存活 chunk,重新编号安全)。显示编号 `#N = seq + 1`(seq 从 0):存活 chunk 期间单调、删除不回收;`pasted` 清空后从 `#1` 重计。

**提交(展开)**:提交时 prompt MUST 取 `expand_folds`(逐 char 把 sentinel 替回 `PastedChunk.text`)后的完整文本;命令旁路判据 MUST 计入 fold(`input().contains('\n') || input_has_fold()` 为真时不试 `parse_command`)。`PushSubmitted` MUST 以展开文本入 history,并清空 `pasted` + 置 `next_paste_seq = 0`(故 `↑` 召回显示展开文本、下一条 `#N` 从 `#1`(seq 0)起)。

**渲染(label dim,跨软换行分段)**:render 端 SHALL 以 `expand_for_display`(sentinel → label,**同时产出各 label 在 display 串中的字节区间**,区间有序、互不重叠、落在 char 边界)+ 光标偏移映射喂 `visual_input_layout`;`InputVisualLayout` SHALL 纯加法暴露 `line_starts`(各可视行在 display 串中的起始字节偏移),不变量:`lines.len() == line_starts.len()` 且 `display[line_starts[i] .. line_starts[i] + lines[i].len()] == lines[i]`。渲染 MUST 以可视行区间与 label 区间**相交**切分 span:label 段渲 `text_muted`、其余正文渲既有 `text_primary`,label 被软换行切开时每一段都 MUST dim;dim 判定 MUST 基于区间而非文本模式匹配(用户手打同款字面文本 MUST NOT 被 dim)。换行与 `input_content_height_cap` 按 label 宽度计入。空 `pasted` 时输入渲染 MUST 与折叠前一致(既有输入快照零 churn);dim 不改文本,既有折叠快照文本亦零 churn。

**Non-Goals(v1)**:不支持 `↑` 编辑/回折已提交的粘贴;不改 `apply_batch_input_key` 既有逐键路径;不保证 `↑↓` 垂直移动跨越/邻接 fold 时光标落位列与屏幕 label 宽度对齐(reduce 按 buffer 列 sentinel=1 算,label ~26 列,v1 接受、真机勿当回归);粘贴文本内**字面 PUA 字符**(U+E000..U+F8FF)与 sentinel 的理论撞车为接受边界(v1 即存在;seq 删空归零使复用窗口略扩,前提同为剪贴板含 PUA,一并接受),不处理。

#### Scenario: 大段粘贴(≥15 行)折叠为占位符 token

- **WHEN** 一批粘贴事件重建出 20 逻辑行文本,`process_event_batch` 时无模态
- **THEN** `fold_candidate` 返回 `Some(该文本)`;施 `InsertPasteFold` 后 `input_line.text` 在光标处含**一个** sentinel、`pasted` 有一项(`line_count=20`)、`next_paste_seq=1`;输入框该处渲为一行 `[Pasted text #1 +20 lines]`,不逐行撑满

#### Scenario: 单行超长粘贴按字符阈值折叠

- **WHEN** 一批粘贴事件重建出**无换行**的 600 字符单行文本(全文本内容键),`process_event_batch` 时无模态
- **THEN** `fold_candidate` 返回 `Some`(`行数 1 < 15` 但 `字符数 600 ≥ 500`);折叠后 `pasted` 一项(`line_count=1`);label 渲为 `[Pasted text #1 +600 chars]`;提交展开为原 600 字符

#### Scenario: 小段粘贴与手打多行不折叠

- **WHEN** 批重建出 14 逻辑行且总字符数 < 500(或手打的多行)
- **THEN** `fold_candidate` 返回 `None`;走既有逐键路径,文本逐字符/逐行进缓冲,`pasted` 为空、渲染同现状

#### Scenario: 折叠触发纯函数(可单测)

- **WHEN** 对 `fold_candidate(batch, 15, 500)` 分别给:14 个裸 `Enter`(+若干 `Char`,重建 15 逻辑行)的纯粘贴批、13 个裸 `Enter`(14 逻辑行、总字符 < 500)批、单行 600 字符批、单行**恰 500** 字符批、单行 499 字符批、14 行 × 40 字符(560)多行批、含 `PageUp` 的混批、空批
- **THEN** 依次:`Some`(行数达标)/ `None`(两阈值均不达)/ `Some`(字符达标)/ `Some`(`≥` 含边界)/ `None` / `Some`(行数不达、字符达标;折叠后 label 仍按 `line_count` 分派为 `+14 lines`)/ `None`(含非文本内容键)/ `None`;边界口径:N 逻辑行 = N−1 个裸 Enter(无尾随)、`count = 裸 Enter 数 + 1`

#### Scenario: 占位符为原子——方向键整体跨过、退格整体删除

- **WHEN** 缓冲为 `a⟦sentinel⟧b`(⟦⟧ 为一个折叠占位符),光标在末尾;先按 `MoveLeft` 两次,再于 sentinel 后按 `Backspace`
- **THEN** `MoveLeft` 一次跨过 `b`、再一次整体跨过占位符(光标落 `a` 后);`Backspace` 于 sentinel 后整体删除该占位符(不进入内部)、随后 `prune_pasted` 使 `pasted` 移除该项

#### Scenario: 删空后编号复位

- **WHEN** 缓冲仅含一个 fold(seq 0),`Backspace` 删除它后再粘贴一段可折叠文本
- **THEN** 删除后 `pasted` 为空且 `next_paste_seq == 0`;新 fold sentinel 复用 U+E000、label 显示 `#1`

#### Scenario: 历史召回往返保留 fold、退出召回弃 draft

- **WHEN** 缓冲含一个 fold,`↑` 召回历史条目后:一路 `↓` 还原(其后再 `Backspace` 删除该 sentinel);另一路直接打字
- **THEN** 还原路:`text` 复原含 sentinel,chunk 完好、label 正常渲染、提交可展开(`prune_pasted` 保留集含 `draft`,召回途中不杀 chunk),且还原**消费** draft(`draft` 为空);随后删除 sentinel → `pasted` 空、`next_paste_seq == 0`(无 stale draft 引用致 zombie);打字路:`exit_history` 清空 `draft`、其独占 chunk 被裁剪、`history_cursor == None`,`text` 中仍存在的 sentinel(若有)不受影响

#### Scenario: SetText 整体替换清孤儿

- **WHEN** `pasted` 持有 chunk(被 `text` 或 `draft` 引用)时发生 `SetText`(如命令补全整体替换输入)
- **THEN** `draft` 被清空、`prune_pasted` 以新 `text` 为准裁剪;新文本不含 sentinel 时 `pasted` 为空且 `next_paste_seq == 0`

#### Scenario: 提交展开为完整文本、history 存展开文本

- **WHEN** 缓冲为 `看这段:⟦#1(20 行原文)⟧`(单 sentinel、无字面 `\n`),按 Enter 提交
- **THEN** prompt = `看这段:` + 20 行原文(`expand_folds`);因含 fold **不**试 `parse_command`;transcript/history 收展开全文;提交后 `pasted` 空、`next_paste_seq=0`;`↑` 召回该条显示展开文本

#### Scenario: 混排多占位符按位置展开保序

- **WHEN** 缓冲为 `⟦#1⟧ 中间 ⟦#2⟧`,两占位符各自原文 A、B
- **THEN** `expand_folds` 得 `A 中间 B`(保序、各归各位);渲染为两个 label 按位置保序(各自形态按其 `line_count` 分派:多行 `+M lines`、单行 `+K chars`)

#### Scenario: line_starts 不变量(可单测)

- **WHEN** 对多逻辑行、软换行(宽度触发)、CJK 宽字符折行、空逻辑行、行恰满(cursor 行末溢出空行)诸 case 调 `visual_input_layout`
- **THEN** 均满足 `lines.len() == line_starts.len()` 且逐行 `text[line_starts[i] .. line_starts[i] + lines[i].len()] == lines[i]`(空行对空串平凡成立);既有 `lines`/`cursor` 断言零改动

#### Scenario: label dim 分段上色(跨软换行,带色断言)

- **WHEN** 输入为 `正文 + fold + 正文` 混排,视口宽使 label 软换行为两段;另有用户**手打**字面 `[Pasted text #1 +2 lines]` 的对照输入
- **THEN** label 两段所在 cell 的 fg 均为 `text_muted`,前后正文 cell 为 `text_primary`(按 `TestBackend` buffer cell 断言,主题无关按 token 比对);手打字面文本 cell 保持 `text_primary`(dim 判定基于 label 字节区间,非文本匹配)

#### Scenario: 折叠渲染与高度核算(insta 快照)

- **WHEN** 输入框含 `前缀文字 [Pasted text #1 +20 lines] 后缀文字`,`TestBackend` 渲染;另测窄宽两宽度与单行 `+K chars` label
- **THEN** 占位符渲为一行 label、与正文可辨;`visual_input_layout` 按 label 宽度换行、`input_content_height_cap` 不因 fold 偷 transcript 地板;空 `pasted` 布局同现状;既有快照零 churn,单行 label 快照新增锁定
