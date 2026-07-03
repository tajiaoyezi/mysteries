## Context

输入缓冲(`src/tui/input_buffer.rs`):`InputBufferState { text: String, cursor: usize, .. }`,`cursor` 为字节偏移;`reduce_input_buffer` 处理 `InsertChar/InsertStr/InsertNewline/Backspace/Delete/Move*/Up/Down`。光标导航与删除均经 `previous_char_boundary`/`next_char_boundary`——**按 char 边界**操作(单 char 是最小原子)。

粘贴路径:`run_tui`(mod.rs:132-195)只做 `drain_event_batch(ev0)`(含 paste-guard 跨批续读)→ 把整批交独立函数 **`process_event_batch(batch, ..)`(mod.rs:697-799,按值持 `batch: Vec<Event>`)**;逐键分类(`press_key_events(&batch)`/`classify_key_batch`,mod.rs:717-718)与逐键循环(`for event in batch` → `apply_batch_input_key`,mod.rs:724+)都在 `process_event_batch` 里。一批 paste 事件里,`KeyIntent::Passthrough` 的 `Char` 攒进 `pending_str`,`KeyIntent::Newline`(n≥2 的裸 Enter,分类见 `input_batch.rs`)触发 `flush_merged_input_chars`(把 `pending_str` 作一个 `InsertStr`)+ `insert_newline_and_refresh`。**关键**:多行粘贴逐行拼入,**无任何单点持有整块粘贴文本**——故折叠检测必须放 **`process_event_batch` 顶部**(逐键循环之前对整批重建),不能挂在 flush。

提交(`on_key_inner` Enter 分支,app.rs:1069-1094):`prompt = self.input().trim()`;`contains_newline = self.input().contains('\n')`;`!contains_newline` 时试 `parse_command`;`PushSubmitted(prompt)` 存 history + 清缓冲。

渲染(`render.rs` `render_input`:1127):`visual_input_layout(state.input(), cursor, width)` 把 `&str` 按 `char_width` 换行成 `lines: Vec<String>`;`input_content_lines` 每条可视行整体 `Span::styled(text.clone(), text_style)`(**单一样式**);高度经 `input_content_height_cap`。

## Goals / Non-Goals

**Goals:**
- 大段粘贴(仅粘贴、≥15 逻辑行)在输入框折叠成一行占位符 `[Pasted text #N +M lines]`;可与手打文字混排;提交时原位展开进 transcript。
- 占位符是**原子**:整体跨过/删除、不可进入内部编辑;**不改动现有光标/退格 reduce 逻辑**。

**Non-Goals:**
- 不做 `↑` 编辑/回折已提交的粘贴(history 存展开文本,召回即展开)。
- 不做单行超长(无换行)折叠(触发按**行数**;字符阈值后续按需另加)。
- 不保证 `↑↓` 垂直移动跨越/邻接 fold 时光标落位列与屏幕 label 宽度对齐(见 Risks)。
- 不改 `apply_batch_input_key` 既有逐键路径(paste-guard 三部曲调过,保持不动;折叠是其**之前**的批级短路)。
- markdown 渲染、diff 高亮不在本 change(同线程各自另开)。

## Decisions

- **D1 存储:PUA 单字符 sentinel + 旁挂 `pasted: BTreeMap<char, PastedChunk>`。** `PastedChunk { seq: u32, text: String, line_count: usize }`;sentinel = `char::from_u32(0xE000 + seq).unwrap()`。选单字符是因为**现有按 char 边界的光标/删除逻辑对单 char 天然原子**——`MoveLeft/Right` 整体跨过、`Backspace/Delete` 整体删掉、光标永不落入内部,`reduce_input_buffer` 的 `Move*/Backspace/Delete` 分支**一行不改**。`BTreeMap` 取其确定迭代序(测试稳定)。`seq` 兼作**显示编号基**与 sentinel 分配源:**`#N = seq + 1`**(seq 从 0 → 首块渲 `#1`,渲染时 +1);提交/清空时 `next_paste_seq` 归零 → 每条消息内 `#1、#2…`。

- **D2 触发:批级纯函数 `fold_candidate(batch: &[Event], threshold: usize) -> Option<String>`(入 `input_batch.rs`)。** **从严口径**——仅当 `press_key_events(batch)` **逐键全为** `is_text_content_key`(`Char` 非纯 Ctrl + 裸 Enter,整批本质是一段纯粘贴)时,重建串(`Char`→字符、裸 `Enter`→`\n`)并判逻辑行数(`split('\n').count()`,= 裸 Enter 数 + 1)`≥ threshold`(`PASTE_FOLD_MIN_LINES=15`)→ `Some(重建串)`;只要出现任一非文本内容键(导航/命令键,如 PageUp)或行数不达阈值,一律 `None`,退回逐键。实现:`keys.iter().all(is_text_content_key)` 再判行数。

- **D3 接线:`process_event_batch` 顶部批级短路(非 `run_tui`)。** 短路点在 `process_event_batch`(mod.rs:697-799)最前——debug 日志与 `press_key_events`/`classify_key_batch`(:717-718)之后、`for event in batch` 逐键消费(:724)之前。若 `state.pending_permission.is_none() && state.models_picker.is_none()` 且 `fold_candidate(&batch, PASTE_FOLD_MIN_LINES)` 命中 → 调 `state.insert_paste_fold(text)` 并**消费整批**(`return Ok(false)`,不进逐键 `apply_batch_input_key` 循环);否则走既有逐键路径。**可见性**:`apply_input_action`(app.rs:532)与 `refresh_command_completion`(app.rs:694)是 `AppState` 私有 `fn`,对父模块 mod.rs 不可见——故新增 `pub(crate) fn insert_paste_fold(&mut self, text: String)`(app.rs)封装二者,mod.rs 只调此入口(比照既有跨模块入口 `flush_merged_input_chars`/`apply_batch_input_key` 均为 `pub(crate)`)。折叠批不含裸 Enter 提交问题:大段粘贴 n≫2,裸 Enter 皆判 `Newline`(已并入重建文本);paste-guard 跨批续读在上游 `drain_event_batch`(mod.rs:656-684,先于 `process_event_batch`),短路点不触碰它。

- **D4 提交展开。** `on_key_inner` Enter 分支:`prompt = self.input_line.expand_folds().trim()`(即 D1/任务 1.2 的 `expand_folds`,全篇统一此一名:对 `input_line.text` 逐 char,sentinel 替 `PastedChunk.text`,其余原样);命令旁路判据改为 `has_multiline = self.input().contains('\n') || self.input_has_fold()`(`input_has_fold() = !self.input_line.pasted.is_empty()`)——含 fold 一律不试 `parse_command`(防单 sentinel 无字面 `\n` 时把展开的多行粘贴误当命令)。**顺序**:先算展开 `prompt`,再 `PushSubmitted(prompt)`(其 reduce 清 `pasted`/text),不可颠倒。`PushSubmitted` 收展开文本 → history 存展开串;reduce 的 `PushSubmitted` 额外清 `pasted` + `next_paste_seq=0`。

- **D5 渲染:显示展开 + 光标映射,喂现有 layout;label dim 样式(带兜底)。** render 端(不改 `visual_input_layout` 签名):
  - `display_text = expand_for_display(text, &pasted)`(sentinel → `[Pasted text #N +M lines]`);`display_cursor = map_cursor(text, cursor, &pasted)`(累加:sentinel 前的字节按 label 字节长、其余按原 char 长)。
  - `visual_input_layout(&display_text, display_cursor, width)` 不变;`input_content_height_cap`/换行按 label 宽度自然算(label 全 ASCII、`char_width`=1)。空间/高度核算复用现状,**无需改 cap 公式**(label 已计入 display_text 长度)。
  - **样式**:目标是 label 以 `text_muted` dim 显示。现 `input_content_lines` 整行单样式,要区分 label 需在 draw 层按显示列区间拆 `Span`。**取舍**:v1 优先做到 label 与正文可辨——`[Pasted text …]` 方括号本身已具辨识度;若"跨软换行的 label 分段上色"实现需侵入已测 `visual_input_layout`(违"纯加法"),则 v1 落**整体正文样式的 label**(不 dim),accent/dim 上色作后续 polish。实现时先试 draw 层按 label 显示区间拆 span(不动 layout),可行即上 dim,不可行即兜底。快照锁定最终呈现。

- **D6 编辑与孤儿清理。** `InsertPasteFold(s)`(reduce):`exit_history`;`seq=next_paste_seq`;`sentinel=PUA(0xE000+seq)`;`line_count=s.split('\n').count()`;`text.insert(cursor, sentinel)` + `cursor+=sentinel.len_utf8()`;`pasted.insert(sentinel, chunk)`;`next_paste_seq+=1`。`Backspace/Delete` 分支末尾调 `prune_pasted()`——`pasted.retain(|c,_| text.contains(*c))`,删掉的占位符整体消失、map 不留孤儿。`#N = seq + 1`(seq 存储、渲染时 +1;删 #1 后再粘为 #2,消息内单调,不回收)。

## Alternatives considered

- **折叠检测挂 `flush_merged_input_chars`**——粘贴逐行 flush,`pending_str` 永不含整块,无法判总行数。改批级重建。弃。
- **累加 `\n` 进 `pending_str`、末次 flush 判折叠**——需改 `apply_batch_input_key` 的 Newline 逐键路径(paste-guard 调过),违"纯加法"且有回归风险。弃。
- **多 sentinel 用同一 char + 按出现序键映射**——删中间项后"第 k 个出现"与原 `#N` 错位,`#N` 稳定性差。改每 chunk 唯一 PUA char。弃。
- **占位符独占整行、不与文字混排**——用户明确要混排(`前缀 ⟦#1⟧ 后缀`)。弃。
- **`↑` 召回回折**——需持久化 fold 元数据进 history,复杂;v1 history 存展开文本、召回即展开。弃(记 Non-Goal)。

## Risks / Trade-offs

- **未折叠小块含 PUA 撞码**:未达阈值的粘贴/手打若含 `0xE000+` 区字符,且与某已分配 sentinel 同码,会被误当占位符渲染/展开。概率极低;v1 记为已知风险,廉价兜底 = 未折叠 `InsertStr` 前剥离/替换 PUA 私有区字符(实现可选上)。
- **label 跨软换行样式**:窄输入框下 label(~28 列)可能被软换行拆两段;dim 上色若要求跨段精确,需 layout 暴露每可视行源字节区间(非纯加法)。见 D5 兜底:v1 可先不 dim。
- **`↑↓` 垂直移动列语义**:reduce 的 Up/Down 按 buffer 文本算目标列,sentinel `char_width=1`(width.rs;`0xE000` 落非宽字符空档),而屏幕 label ~26 列 → 跨/邻 fold 行做垂直移动时,落位列(按 sentinel=1)与屏幕视觉列(label 宽)不一致,光标可能跳到视觉对不上处。**v1 接受**(记 Non-Goal,真机复核勿当回归):要一致需 Up/Down 也走 display 列映射,侵入已测 reduce、违纯加法,不做。原子性不受影响(光标恒落 char 边界、不入 sentinel 内部)。
- **`#N` 不回收**:删 #1 再粘为 #2(消息内单调)。可接受,合"每条消息内单调"语义。
- **history 膨胀**:提交存展开文本,大段粘贴令 history 条目变大;与现状(粘贴即入 history)一致,无回归。
- **阈值手感**:`PASTE_FOLD_MIN_LINES=15` 真机可调;过小则小段代码也折叠、过大则大段仍刷屏。
