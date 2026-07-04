# Design — polish-paste-fold

## Context

add-paste-fold 的存量机制:PUA 单字符 sentinel 插入 `text`、旁挂 `pasted: BTreeMap<char, PastedChunk>`;渲染经 `expand_for_display`(sentinel → label 字符串)喂 `visual_input_layout`(折行,返回 `lines: Vec<String>`);提交经 `expand_folds` 展开后 `PushSubmitted`(history 存展开文本,map 清空、seq 归零)。本 change 不动这套骨架,只补三个卫生面。

## D1 label dim:区间法(layout 暴露 line_starts)

- **选**:两个纯加法数据源相交——
  - `expand_for_display` 改为返回 `DisplayExpansion { text: String, label_ranges: Vec<Range<usize>> }`(构造 display 串时顺手记录每个 label 的字节区间;区间天然**有序、互不重叠、落在 char 边界**);
  - `InputVisualLayout` 加字段 `line_starts: Vec<usize>`(各可视行在 display 串中的起始字节偏移)。折行是逐字符推进的,每个可视行都是 display 串的**连续子串**,偏移良定义。不变量:`lines.len() == line_starts.len()` 且 `display[line_starts[i] .. line_starts[i] + lines[i].len()] == lines[i]`(空逻辑行 start = 行首偏移;行恰满时 cursor 溢出产生的空行 start = 行末偏移,不变量对空串平凡成立)。
  - 渲染时每可视行以 `[line_starts[i], line_starts[i] + lines[i].len())` 与 `label_ranges` 相交,切出 `Vec<Span>`:label 段 `text_muted`、其余 `text_primary`。切分点均为区间端点的 max/min,双方都在 char 边界 → 字节切片安全。
- **弃**:
  - 每可视行按 `[Pasted text #N ...]` 文本模式匹配后上色——label 被软换行切成两段后模式失配(正是需要 dim 的窄视口场景);用户手打同款字面文本会被误 dim。
  - sentinel 直通 layout(折行函数自己展开 label 并感知其身份)——侵入已测 wrap 逻辑,违纯加法;v1 当时正是因此兜底。
- 现 spec 已预留该方向(「dim 为目标……由锁定快照为准」),本 change 把它从兜底升级为契约。
- 非 label 行退化为单 span,分配次数与现状一致;`display_cursor`、滚动(`enumerate().skip(scroll)`,`visual_row` 为绝对行号,可直接索引 `line_starts`)不变。

## D2 单行超长折叠:OR 触发 + `+K chars` label

- 触发:`fold_candidate(batch, min_lines, min_chars)`,语义 = 全文本内容键 AND(`split('\n').count() >= min_lines` OR `chars().count() >= min_chars`)。`PASTE_FOLD_MIN_CHARS = 500` 与 `PASTE_FOLD_MIN_LINES` 同置 `input_batch.rs`。
- **500 的取值**:URL / 长路径(~100–300 字符)应保持可见可编辑;minified / base64 / 整行日志(≥500)折叠。误触发面≈0:打字不可能在单个 drain 批凑出 500 字符,粘贴合批(`PASTE_COALESCE_*`)才会。真机手感项,可与其余三阈值一并调。
- label:**按 chunk 的 `line_count` 分派、与触发原因无关**——`line_count >= 2` → `+M lines`(含"行数不达、字符达标"的多行 chunk,如 14 行 × 40 字符);`line_count == 1` → `+K chars`,K = `chunk.text.chars().count()`(字符数非字节数,CJK 正确——与「已复制 N 字」同口径)。
- **弃**:`PastedChunk` 扩 `char_count` 字段缓存——渲染现算是每帧 ~4 次 O(len) 线性扫(`expand_for_display` 两 callsite + `display_cursor` 两处),chunk 尺寸受 `EVENT_BATCH_CAP` 约束,数百 KB 亦 ~1ms 级;省一处结构变更与既有测试构造点 churn。真机若见卡顿再缓存(有 profile 依据才动)。
- 既有 `fold_candidate` 测试(14 行 case 等)字符数远小于 500,迁签名后语义零变。

## D3 prune 保留集 text ∪ draft + draft 生命周期

- **为什么 text-only 不行**:`history_up` 把当前输入(可能含 sentinel)存入 `draft`、`text` 换成历史条目;若此刻按 text-only 裁剪,draft 引用的 chunk 被杀,`history_down` 还原后 sentinel 失配——渲染成裸 PUA 字符、提交丢原文,**数据丢失级**。故保留集 = `text ∪ draft` 中出现的 sentinel。
- **draft 弃置点**:draft 的还原路径(`history_down` 的 Some→None 分支)仅在 `history_cursor.is_some()` 时可达。三处使它永久不可达的动作 MUST 弃 draft(清空 + `prune_pasted`):
  - `exit_history`(召回态打字/退格):仅原 `history_cursor.is_some()` 时清 draft + prune → 正常打字热路径零额外开销;
  - `SetText`(命令补全整体替换):**无条件**清 draft + prune(无论是否召回中,替换后 draft 均无消费者);
  - `history_down` 还原分支(Some→None):**消费即清空**——`text = draft` 后置空 draft 再 prune(还原后 cursor 已 None、还原路径不可达,draft 唯一后续用途是被下次 `Up` 覆写)。不清则产生审查坐实的 zombie 流:fold → `↑` → `↓` 还原 → `Backspace` 删 sentinel,stale draft 仍引用该 sentinel → chunk 存活、`pasted` 非空、编号不归零,下一条 fold 渲 `#2`。
- **prune 调用点**:既有 `Backspace`/`Delete` 保持;新增 `SetText`、`history_up`、`history_down`(还原分支 text == draft,裁剪幂等无害)。
- **seq 归零**:`prune_pasted` 裁后若 `pasted.is_empty()` 则 `next_paste_seq = 0`。仅在无任何存活 chunk 时归零,sentinel 复用无撞车面;有存活 chunk 时保持单调(与现契约一致)。效果:同一条消息内删光 fold 再粘,编号从 `#1` 重计。
- **弃**:每个 reducer 动作末尾统一 prune——`InsertChar` 等热路径为不存在的孤儿反复扫描,无谓;孤儿只可能产生于文本整体替换与 sentinel 删除,按点位裁剪即全覆盖。
- 借用检查:`prune_pasted` 内 `let text = &self.text; let draft = &self.draft;` 与 `self.pasted.retain(...)` 为不相交字段借用,与现实现同型。

## D4 快照与测试策略

- **零 churn 面**:dim 不改文本 → `tui_paste_fold_input` 等既有快照文本不变;`+K chars` 仅出现在新增测试。若执行中出现任何既有快照 churn,即为回归信号,停下报告而不是接受快照。
- **带色断言**:沿用 add-diff-highlight 模式(按 `TestBackend` buffer cell 的 fg 与 theme token 比对,主题无关):label cell == `text_muted`、正文 cell == `text_primary`、跨软换行两段均 dim、手打字面 `[Pasted text #1 +2 lines]` 不 dim。
- **TDD 边界**:`fold_candidate` 双阈值、`prune_pasted`/draft 生命周期、`line_starts` 不变量、label 区间产出与 span 切分 helper = 纯逻辑,**强制红绿**;`render_input` 接线 = TUI 外壳,事后快照 + 带色断言。

## Non-Goals / 已知边界

- 粘贴文本内**字面 PUA 字符**(U+E000..)与 sentinel 的理论撞车为接受边界(v1 即存在;seq 删空归零使 sentinel 复用窗口略有扩大,前提同样是剪贴板含 PUA,一并接受),本 change 不处理。
- `↑` 编辑/回折已提交粘贴、`↑↓` 垂直移动列与 label 宽对齐:维持 v1 Non-Goal。
- daylight 主题下 `text_muted` 的可辨度:真机核验项,若过淡再升 `text_secondary`(单点改色)。

## Risks

- `expand_for_display` 返回类型变化波及两个 callsite(`input_box_height`/`render_input`)与若干测试——机械改造,以编译器驱动。
- `line_starts` 与 `lines` 失配 → dim 错位:靠不变量测试(多逻辑行 + 软换行 + CJK + 空行 + 行恰满溢出)整面锁死。
