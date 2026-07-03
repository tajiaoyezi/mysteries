# 2026-07-03 · 40 · archive add-paste-fold

## 决策
- **粘贴折叠占位符:PUA 单字符 sentinel + 旁挂 `pasted: BTreeMap<char, PastedChunk>`** | 选:`text` 里插一个私有区单字符(`char::from_u32(0xE000+seq)`),原文旁挂 map | 弃:整框单块折叠(不能混排)、同一 sentinel 按出现序键映射(删中间项 `#N` 错位) | 依据:code——单 char 天然被现有 `previous/next_char_boundary` 光标/删除逻辑当一个原子,`Move*/Backspace/Delete` 分支一行不改
- **触发:批级纯函数 `fold_candidate` 从严** | 仅当整批 press 键全为 `is_text_content_key` 且 `split('\n').count() >= 15` 才 `Some` | 弃:挂 `flush_merged_input_chars`(粘贴逐行 flush、无单点持整段) | 依据:tests
- **显示编号 `#N = seq + 1`** | seq 从 0、渲染 +1 → 首块 `#1` | 主导:第一轮审查 finding(四处 artifact 曾自相矛盾、两份快照 fixture 互斥) | 依据:spec/快照
- **label 样式 v1 = 正文色(不 dim)** | design D5 兜底:dim 需 layout 暴露每可视行源字节区间(违纯加法),v1 方括号已辨识 | 待决:dim 作后续 polish
- **★ 根因修正(真机发现):`drain_event_batch` 粘贴合批** | **per-batch 折叠检测对跨批粘贴是盲的**——ConPTY 把大粘贴切成 ~32 字符/批的多个 chunk 分次投递(debug 探针实测:一次粘贴打出 `len=64/2/27/1` 多个 `fold=false` 小批、全 `Char` 无 `Enter`),`fold_candidate(&batch)` 每批不足 15 行 → 永不折叠 | 修:批已像粘贴(`batch.len() >= PASTE_COALESCE_MIN_EVENTS=8`)时用 `PASTE_COALESCE_GRACE=30ms` 桥接后续 chunk 并成一批,`fold_candidate` 遂见整段;保留 lone-enter 10ms 续读分支,两分支都靠 `if !is_key break` 防高频非键事件死循环 | 主导:systematic-debugging(先 debug 探针取证再定罪) | 依据:真机日志。**这是我设计时的盲区**:假设"batch = 整段粘贴",与 guard-paste-cross-batch 已坐实的"大粘贴跨批"矛盾,两轮 propose 审查查了接线/一致性却漏了这个经验前提

## 变更
- `src/tui/input_buffer.rs`:`InputBufferState` 加 `pasted`/`next_paste_seq` + `PastedChunk`;`InsertPasteFold` 动作;`expand_folds`/`prune_pasted`;`PushSubmitted` 清 map+归零;Backspace/Delete 末尾 prune。
- `src/tui/input_batch.rs`:`PASTE_FOLD_MIN_LINES=15` + `fold_candidate`(从严:全为文本内容键 + 行数)。
- `src/tui/mod.rs`:`process_event_batch` 顶部批级折叠短路(`insert_paste_fold`+`return Ok(false)`);**`drain_event_batch` 粘贴合批**(`PASTE_COALESCE_MIN_EVENTS`/`PASTE_COALESCE_GRACE`)。
- `src/tui/app.rs`:`pub(crate) insert_paste_fold`(封装私有 `apply_input_action`+`refresh_command_completion`);Enter 先 `expand_folds` 后 `PushSubmitted`、命令旁路计入 `input_has_fold`。
- `src/tui/render.rs`:`expand_for_display`/`display_cursor`/`fold_label` 喂现有 `visual_input_layout`(两 callsite:`input_box_height` + `render_input`);label 正文样式。新快照 `tui_paste_fold_input`,既有快照零 churn。
- 全库 `cargo test` 470 passed;clippy 净;真机正常模式折叠生效、<1s。

## 待决
- **~~`s` 泄漏~~(已修,真 bug)**:后于正常模式复现(`ys` 落折叠块外),**非 debug 产物**——根因:粘贴首个小 chunk(如 `ys`=4 事件)`< PASTE_COALESCE_MIN_EVENTS(8)` 被漏判成打字、先逐键插入,之后大 chunk 才合批折叠。修:阈值 **8→4**(单键仅 2 事件,4 为不误触打字的下限)。cosmetic(提交展开仍完整,非数据丢失)。**残留**:首 chunk 恰 1 字符(2 事件,与单键无法区分)仍漏 1 字符,固有边界。
- **~~首折 `#2`~~**:仅 debug 模式出现(每事件文件 I/O 反压 ConPTY → 合批中途截断成两折);正常模式为单折 `#1`,确系 debug 时序产物。
- **2s 卡顿**:亦 debug I/O 所致,正常模式 <1s;drain 阻塞合批对超大粘贴的固有代价,暂可接受。
- **label dim 样式**、**`↑` 编辑排队/回折**、**单行超长按字符阈值折叠**、**`↑↓` 垂直移动列与 label 宽对齐**:均 v1 Non-Goal / 后续。
- `PASTE_COALESCE_MIN_EVENTS=8` / `GRACE=30ms` / `PASTE_FOLD_MIN_LINES=15` 三阈值真机手感可再调。

## 引用
- OpenSpec change:`add-paste-fold`(propose `a52dd59`,两轮对抗审查 9+1 CONFIRMED 收敛)。
- 相关 log:[[2026-07-02-38-archive-guard-paste-cross-batch]](同坐实"大粘贴跨批"、本次盲区之源)、[[2026-07-03-39-archive-add-message-queue]]。
- 环境坑:[[bash-tool-not-powershell-heredoc]](commit message 用 heredoc)。
