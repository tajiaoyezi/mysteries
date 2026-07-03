## 1. 缓冲折叠状态 + 动作(纯逻辑 · RED 停点)

- [x] 1.1 `src/tui/input_buffer.rs`:`InputBufferState` 加 `pasted: BTreeMap<char, PastedChunk>`(初始空)、`next_paste_seq: u32`(初始 0);`PastedChunk { seq: u32, text: String, line_count: usize }`。新 `InputBufferAction::InsertPasteFold(String)`。reduce:`exit_history`;`seq=next_paste_seq`;`sentinel=char::from_u32(0xE000+seq)`;`line_count=s.split('\n').count()`;`text.insert(cursor, sentinel)`+`cursor+=sentinel.len_utf8()`;`pasted.insert(sentinel, chunk)`;`next_paste_seq+=1`。**先只写单测跑红**(新接口,贴红停等确认):InsertPasteFold 后 text 含 sentinel、cursor 在其后、`pasted` 有一项(seq=0、line_count 对)、`next_paste_seq=1`;连折两段 → 两个不同 sentinel、seq 0/1。再实现转绿。
- [x] 1.2 `InputBufferState::expand_folds(&self) -> String`(**方法形式为唯一签名**,与 3.2/D4 的 `self.input_line.expand_folds()` 一致——同一 `InputBufferState` 上同时读 `self.text` 与 `self.pasted`):逐 char,sentinel(map 键)替 `chunk.text`,map 无该键的字面 sentinel(被 prune 后残留,理论不出现)原样保留;其余原样。`prune_pasted(&mut self)`:`pasted.retain(|c,_| text.contains(*c))`。`PushSubmitted` reduce 末尾清 `pasted` + `next_paste_seq=0`。`Backspace`/`Delete` reduce 末尾调 `prune_pasted`。**先写单测跑红**(`expand_folds`/`prune_pasted` 为新接口,贴红停等确认):expand 混排(`a⟦#1⟧b` → `a<原文>b`)、多占位保序;Backspace 删 sentinel 后 `pasted` 空;PushSubmitted 后 `pasted` 空且 `next_paste_seq=0`。再实现转绿。

## 2. 触发重建(纯逻辑 · RED 停点)

- [x] 2.1 `src/tui/input_batch.rs`:`const PASTE_FOLD_MIN_LINES: usize = 15;`;`fold_candidate(batch: &[Event], threshold: usize) -> Option<String>`——**从严口径**:仅当 `press_key_events(batch)` **逐键全为** `is_text_content_key`(整批本质是一段纯粘贴)时,重建串(`Char`→字符、裸 `Enter`→`\n`)并判 `split('\n').count() >= threshold` → `Some(串)`;只要出现任一非文本内容键(导航/纯 Ctrl 等,如 PageUp),或行数不达阈值,一律 `None`。实现先 `keys.iter().all(is_text_content_key)` 再判行数。**行数口径**:N 逻辑行 = N−1 个裸 Enter 分隔(无尾随 Enter);`split('\n').count() = 裸 Enter 数 + 1`。**先写单测跑红**(新接口,贴红停等确认),用例按**裸 Enter 个数**构批:14 个裸 Enter(+Char)→ 重建 15 逻辑行、`count==15` → `Some`;13 个裸 Enter → `count==14` → `None`;含 PageUp 的混批 → `None`;空批 → `None`;重建串内容(含 CJK)正确、裸 Enter 成 `\n`。再实现转绿。

## 3. 接线批级折叠 + 提交展开(process_event_batch / 提交)

- [x] 3.1 接线点在 **`process_event_batch`(`src/tui/mod.rs`,按值持 `batch: Vec<Event>`)顶部**——即 debug 日志与 `press_key_events`/`classify_key_batch` 之后、`for event in batch` 逐键消费之前(**非** `run_tui` 主循环;paste-guard 跨批续读在上游 `drain_event_batch`,不受影响)。若 `state.pending_permission.is_none() && state.models_picker.is_none()` 且 `fold_candidate(&batch, PASTE_FOLD_MIN_LINES)` 命中 → 调 `state.insert_paste_fold(text)`,**消费整批**(`return Ok(false)`,不进逐键 `apply_batch_input_key` 循环);否则走既有逐键路径。**新增 `pub(crate) fn insert_paste_fold(&mut self, text: String)`**(`src/tui/app.rs`)封装私有 `apply_input_action(InsertPasteFold(text))` + `refresh_command_completion`——两私有方法保持私有,mod.rs 只调此 `pub(crate)` 入口(不得从 mod.rs 直调 `apply_input_action`/`refresh_command_completion`,它们对父模块不可见)。
- [x] 3.2 `src/tui/app.rs` `on_key_inner` Enter 分支:`prompt` 取展开串(`self.input_line.expand_folds().trim()`,即 1.2 的 `expand_folds`,统一此一名);命令旁路判据 `!contains_newline` 改为 `!(self.input().contains('\n') || self.input_has_fold())`(`input_has_fold()` = `!self.input_line.pasted.is_empty()`;含 fold 不试 `parse_command`);`PushSubmitted(prompt)` 收展开文本。**顺序**:先算展开 `prompt`,再 `PushSubmitted`(其 reduce 清 `pasted`/text),不可颠倒。空展开串(trim 后空)仍早退。
- [x] 3.3 集成/单测验证:running 态大段粘贴(经批级)→ 入队/直发的 prompt 是**展开文本**、history 存展开文本;单 sentinel(无字面 `\n`)提交**不**误走命令;折叠 + 手打混排提交 → 展开处于正确位置;命令 `/help` 仍即时(小输入不折叠、不含 fold)。

## 4. 渲染折叠区 + 高度核算(TUI 外壳 · 事后快照 + 高度单测)

- [x] 4.1 `src/tui/render.rs`:`expand_for_display(text, &pasted) -> String`(sentinel → `[Pasted text #{seq + 1} +{line_count} lines]`,**显示编号 = seq+1**,seq 从 0 → 首块渲 `#1`)+ `map_cursor(text, cursor, &pasted) -> usize`(sentinel 前按 label 字节长累加、其余按原 char 长);`render_input` 用 `display_text`/`display_cursor` 喂 `visual_input_layout`(高度 cap 复用现状,label 已在 display_text 长度内)。同步 render.rs:97 的高度预算 `visual_input_layout` 调用改用 display_text。
- [x] 4.2 label 样式:先试 draw 层(`input_content_lines`)按 label 显示列区间拆 `Span` 上 `text_muted` dim——不改 `visual_input_layout`;若跨软换行分段上色需侵入 layout(违纯加法),v1 兜底为整体正文样式 label(不 dim),快照锁定实际呈现。高度单测:含 1 个 fold(label ~28 列)、窄/宽两宽度下可视行数与 cursor 位置符合预期;`input_content_height_cap` 不因 fold 偷 transcript 地板。
- [x] 4.3 insta 快照:输入框含 `前缀文字 [Pasted text #1 +20 lines] 后缀文字`(混排,首块 seq=0 → 渲 `#1`);空 `pasted` 时输入渲染同现状(既有输入快照零 churn)。

## 5. 校验

- [x] 5.1 `cargo test --lib` 全绿 + `cargo clippy --all-targets -- -D warnings` 零警告 + `openspec validate add-paste-fold --strict` 通过;**真机复核**:粘贴 ≥15 行 → 折叠成一行占位符、不刷屏、不撑穿输入框;粘贴 <15 行 → 照常展开;`前缀+粘贴+后缀`混排显示正确;左右方向键整体跨过占位符、Backspace 整体删除;提交后 transcript 收到**展开全文**;`↑` 召回上条(含粘贴)显示展开文本;阈值/样式手感确认。
