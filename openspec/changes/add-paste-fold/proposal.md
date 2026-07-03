## Why

大段粘贴现在逐行拼进输入框:`apply_batch_input_key` 把一批 paste 事件按键处理——Passthrough 的 `Char` 攒进 `pending_str`,每遇 `Newline`(n≥2 的裸 Enter)就 `flush_merged_input_chars`(`InsertStr` 当前行)+ `insert_newline`,于是 N 行粘贴 = `InsertStr(行1)+InsertNewline+InsertStr(行2)+…`。行数一多:① 逐行 reduce + 每帧全屏重绘卡顿(log 38 记录的真机痛点);② 输入框被撑满、挤占 transcript;③ 视觉噪声大。参考 Claude Code:大段粘贴在输入框折叠成一行占位符 `[Pasted text #N +M lines]`,提交时才展开进 transcript。

## What Changes

- **触发(仅粘贴 + 超阈值)**:批级检测——纯函数 `fold_candidate(batch, threshold)` 从 press 键重建粘贴文本(`Char`→字符、裸 `Enter`→`\n`),逻辑行数 `≥ PASTE_FOLD_MIN_LINES`(=15)返回 `Some(text)`。手打多行、小粘贴不折叠。
- **存储(原子 token + 旁挂映射)**:`InputBufferState` 加 `pasted: BTreeMap<char, PastedChunk>` + `next_paste_seq: u32`。折叠 = 在 `text` 插一个**私有区单字符** sentinel(`char::from_u32(0xE000 + seq)`);单字符天然被现有 `previous/next_char_boundary` 光标导航当**一个原子**——左右整体跨过、Backspace/Delete 整体删除、不可进入内部,**光标/退格逻辑零改动**。
- **编辑**:可 `前缀文字 ⟦#1⟧ 后缀文字` 混排、可多个占位符;删除 sentinel 后 `prune_pasted()` 清孤儿 map 项。
- **提交(展开)**:`expand_folds()` 把每个 sentinel 替回 `PastedChunk.text`;submit 用展开串当 prompt;`PushSubmitted` 存**展开后**文本进 history(↑ 召回显示展开原文、不回折)+ 清 map + `next_paste_seq` 归零。命令旁路 `contains_newline` 判据加"或含 fold",防单 sentinel 无字面 `\n` 时误走 `parse_command`。
- **渲染**:render 端"显示展开串 + 光标字节偏移映射"两个 helper 喂现有 `visual_input_layout`;`input_content_height_cap`/换行按 label 宽度算;sentinel 渲成 `[Pasted text #N +M lines]`,dim 样式(见 design D5 的样式取舍与兜底)。
- **编号 / 阈值**:`#N` 每条消息内重置、消息内单调(`next_paste_seq` 提交归零);阈值 15 逻辑行,具名常量真机可调。

## Capabilities

### New Capabilities

- `tui-shell`:
  - **ADDED**:`粘贴折叠占位符(大段粘贴折叠为原子 token)` —— 批级触发重建、PUA 单字符原子存储 + 旁挂 `PastedChunk` 映射、混排编辑与孤儿清理、提交展开(history 存展开文本)、输入框折叠渲染与高度核算。

## Impact

- **代码**:
  - `src/tui/input_buffer.rs`:`InputBufferState` 加 `pasted`/`next_paste_seq` + `PastedChunk`;新 `InputBufferAction::InsertPasteFold(String)`;`expand_folds()`;`prune_pasted()`;`PushSubmitted` 清 map + 归零 seq;Backspace/Delete 后 prune。
  - `src/tui/input_batch.rs`:纯函数 `fold_candidate(batch, threshold) -> Option<String>` + 常量 `PASTE_FOLD_MIN_LINES`。
  - `src/tui/mod.rs`:`process_event_batch` 顶部批级——`fold_candidate` 命中且无模态/pending → 调 `state.insert_paste_fold(text)`、`return` 消费整批;否则走现有逐键路径。
  - `src/tui/app.rs`:新增 `pub(crate) fn insert_paste_fold`(封装私有 `apply_input_action`+`refresh_command_completion`);submit 用 `expand_folds()` 当 prompt + 命令旁路判据加 `input_has_fold()`;sentinel 分配/`#N`(=seq+1)经 reduce。
  - `src/tui/render.rs` + `input_layout.rs`:显示展开 + 光标映射,label 渲染 + 宽度/高度核算。
- **依赖**:零新增。
- **测试**:折叠状态/动作、`fold_candidate`、`expand_folds`/prune —— 纯逻辑单测(RED 停点);批级接线 + 提交展开 —— 集成验证;折叠渲染 insta 快照 + 高度回归单测。
- **风险**:见 design(未折叠小块含 PUA 撞码、label 样式跨软换行的渲染取舍、单行超长不折的行数触发口径)。
- **边界**:本 change 只做**折叠占位符**;同属"`tui/` 渲染加法线程"的 **markdown 渲染**、**diff 高亮**各自另开 change,不在本 change。
