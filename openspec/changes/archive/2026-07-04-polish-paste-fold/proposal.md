# polish-paste-fold

## Why

add-paste-fold(v1,log 40)落地后留了三个卫生尾巴,真机长用可感:

1. **label 不 dim**:占位符与正文同色,混排时只靠方括号肉眼辨识。v1 兜底的原因(D5)是 `visual_input_layout` 只返回 `lines: Vec<String>` 与光标位置、不含源偏移,渲染层不知道 label 落在哪个可视行的哪一段——跨软换行分段上色需要 layout 暴露源偏移。
2. **单行超长不折叠**:触发只按逻辑行数(`≥15`),一段几千字符的无换行粘贴(minified JSON / base64 / 长日志行)照样逐字符撑满输入框。
3. **孤儿映射**:`SetText`(命令补全)与 `history_up`/`history_down`(历史召回)整体替换 `text` 时不裁剪 `pasted` map;召回态编辑退出或 `↓` 还原消费后,`draft` 的还原路径永久不可达,其独占 chunk 却滞留到下一次裁剪点(退格/删除或提交),期间还会把编号顶高。`next_paste_seq` 亦只在提交时归零,同一条消息内删光 fold 后再粘,编号不从 `#1` 起。

## What Changes

1. **label dim(区间法)**:`expand_for_display` 同时产出各 label 在 display 串中的字节区间;`InputVisualLayout` 纯加法暴露 `line_starts`(各可视行起始字节偏移);渲染按可视行区间与 label 区间**相交**切 span,label 段渲 `text_muted`,跨软换行的每一段都 dim。判定基于区间而非文本模式匹配(手打同款字面文本不误 dim)。label 文本不变 → 既有快照零 churn,新增带色断言。
2. **单行超长折叠**:`fold_candidate(batch, min_lines, min_chars)` 触发改为 `行数 ≥ PASTE_FOLD_MIN_LINES(=15) || 字符数 ≥ PASTE_FOLD_MIN_CHARS(=500,新常量)`;单行 chunk(`line_count == 1`)的 label 渲为 `[Pasted text #N +K chars]`。
3. **prune 收紧**:`prune_pasted` 保留集改为 `text ∪ draft` 中出现的 sentinel(text-only 会在召回途中杀掉 draft 引用的 chunk,`↓` 还原后 sentinel 失配丢数据);`SetText`/`history_up`/`history_down` 替换文本后调用之;draft 还原路径不可达即弃——`exit_history`(召回态编辑退出)与 `SetText` 清空 draft,`history_down` 还原分支**消费即清空**(`text = draft` 后置空),三处均随手裁剪;`pasted` 清空时 `next_paste_seq` 归零。

## Impact

- Affected specs:`tui-shell`(MODIFIED「粘贴折叠占位符」一条)
- Affected code:`src/tui/input_batch.rs`(触发)、`src/tui/input_buffer.rs`(prune / draft 生命周期)、`src/tui/input_layout.rs`(`line_starts`)、`src/tui/render.rs`(label 区间 + 分段上色 + 单行 label)、`src/tui/mod.rs`(`fold_candidate` callsite 补实参)
- 纯逻辑(触发 / 裁剪 / layout 偏移 / span 切分)强制 TDD 红绿;渲染接线以快照 + 带色断言事后回归
- 既有快照预期**零 churn**(dim 只改色不改文本;`+K chars` 仅出现在新增测试)
