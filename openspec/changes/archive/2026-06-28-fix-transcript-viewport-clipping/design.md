## Context

现状(已对 `src/tui/render.rs` 当前 HEAD=a1cc418 核实):

- `render`(~14)按 `layout_rows` 切六区,transcript 区为 `rows[1]`(`Constraint::Min(8)`)。
- `render_transcript`(~92):取 `visible_transcript_lines(...)` 得到一组 `Line`,塞进 `Paragraph::new(lines).wrap(Wrap{trim:false})` 渲染到 transcript 区。
- `visible_transcript_lines`(~109):先 `transcript_content_lines` 把各 block 摊成 `Vec<Line>`(`message_lines` 对 `User`/`Assistant` 用 `wrap_text` **预换行**),再 `state.visible_scroll_offset(lines.len(), viewport_lines)` 求 offset,最后 `skip(offset).take(viewport_lines)` **精确切出 ≤ viewport_lines 个逻辑行**。
- `wrap_text`(~307)按项目自定义 `char_width`(~868,CJK/emoji=2、组合/零宽=0、其余=1)换行;`Paragraph.wrap` 则按 ratatui 内部 unicode-width 换行。
- **未预换行的行**:`visible_tool_output_lines`(~695)直接 `output.lines()` 不换长行;工具卡头 `tool_card_head`(~580)整行(展开态 args=全 json);固定 **80 宽**边框(工具卡脚 ~573、`error_block_lines` ~482、`help_block_lines` ~332、`status_block_lines` ~394 的 `┌─…`/`└──…`)。

**Bug 机制(根因)**:`visible_transcript_lines` 已按预换行行数精确切出 `viewport_lines` 行交给 `Paragraph`;但 `Paragraph.wrap` 会对这些行**二次换行**。只要某行在两套宽度算法下结果不同(emoji/CJK/符号差 1 格),或本就没预换行(长工具输出 / 80 宽边框在更窄终端),该逻辑行就被拆成 **≥2 屏幕行** → 实际屏幕行数 > 视口高度 → `Paragraph` 自顶向下填、**底部溢出被裁**。因切片末尾正是**最新** `User`/`Assistant`,故最新内容落到视口下方看不见;后续输出使 offset 抬高,旧内容滚出、最新才「顶上来」。

权威次序:行为以 code/test 为准;本 change 据 code 现状定位并修,锁回归测试。

## Goals / Non-Goals

**Goals:**
- 消除 transcript 的双重换行,使「切出的逻辑行数 == 实际屏幕行数 == ≤ 视口高度」。
- `follows_bottom` 时最新(底部)内容在视口内可见(修复主症状)。
- 锁一条回归不变量(spec)+ 一个**复现测试**(bug 在=red,修复后=green)防回潮。

**Non-Goals(本期不做):**
- **不引入 `unicode-width` 依赖**(取舍见决策 ②):去掉二次换行后,残留个别宽字符行尾 1 格差异退化为**右端截断**(横向、非纵向溢出),本期接受。
- 不改 `app.rs` 的 offset/跟随逻辑(本就正确,非 bug 源)。
- 不改 `Paragraph` 之外的布局 / 折叠 / 滚动键行为。

## Decisions

### 决策 ① spec 挂载点:ADDED 新 requirement(而非 MODIFIED 既有)

现有 `tui-shell` spec 中:
- 「transcript 滚动」管 `scroll_offset`/`follows_bottom`/clamp **逻辑**——正确、非 bug 源,改它会误指「滚动逻辑错」。
- 「终端文本排版与宽度度量」管逐块 `User`/`Assistant` 换行 + marker + `display_width` + 光标 + 欢迎居中——是**逐块文本**契约,不含「渲染屏幕行数 == 视口高度 / 不二次换行」这一**跨块渲染不变量**。

二者都**不**覆盖本 bug 要锁的不变量。故 **ADDED** 一条新 requirement「transcript 视口渲染保真」,聚焦「可见屏幕行数对齐视口高度、底部可见、预换行为唯一换行来源」。**备选(弃)**:MODIFY「终端文本排版与宽度度量」——会把「跨块视口保真」塞进「逐块文本」语义,边界模糊;且其既有 4 个 scenario 仍成立,无需改。**待上游确认**(Open Questions Q1):认可 ADDED 挂载,还是倾向并入「终端文本排版与宽度度量」MODIFIED。

### 决策 ② 方案选型:去 `.wrap`(预换行为唯一来源)优于「加 unicode-width 校准」

- **选定(上游已拍板,方案 1)**:删 `render_transcript` 的 `.wrap(Wrap)`。`Paragraph` 无 wrap 时,每个 `Line` 恰好占 **1 屏幕行**(过宽则按 ratatui 在右端**截断**,不换行)。于是「预换行」成为**唯一**换行来源,而预换行发生在 `take(viewport_lines)` **之前** → 切出的行数 == 屏幕行数 == ≤ 视口高度,底部不再溢出。
- **为何不加 `unicode-width`(备选,弃)**:① 这是**架构**问题(两层换行)而非单纯「宽度算不准」——即便 `char_width` 与 ratatui 完全一致,任何**忘记预换行**的行(工具输出、80 宽边框)仍会被 `Paragraph` 二次换行顶高;去掉第二层才是根治。② 加依赖违背「不擅自扩张 dependency」;③ 去 `.wrap` 后,宽度算法差异从「纵向溢出裁底(功能 bug)」降级为「横向右端截断(观感,可接受)」,代价可控。
- **配套(方案 1 第 2 步)**:为保**长内容可读**(去 wrap 后过宽会被截断而非换行),把应当换行的内容补**预换行**:`visible_tool_output_lines` 长行按 `width - display_width("│ ")` 用 `wrap_text` 预换;核对工具卡头 / `Notice` 等。固定边框按 `width` 自适配(决策 ③)。

### 决策 ③ 固定 80 宽边框:按 `width` 自适配

- 工具卡脚 / `Error`/`Help`/`Status` 的 `┌─…`/`└──…` 由固定 80 `─` 改为按渲染 `width` 生成(如 `"─".repeat(width.saturating_sub(k))`),使边框行显示宽度 ≤ 视口宽度、占恰好 1 屏幕行。
- **选定(上游 Q2 拍板)**:本期顺带做边框自适配,窄终端右边框完整、不二次换行顶高。**备选(弃)**:仅去 wrap、接受右端截断——观感缺角,上游否决。

### 决策 ④ 复现测试设计(implement 阶段 🔴 红灯停点)

**性质**:逻辑断言测试(非纯 insta 快照)——快照会把「裁切后的错误布局」也锁住,无法表达「最新内容必须可见」的意图;故用**内容可见性断言**。

**recipe(给 implement,需确保 bug 在时 red)**:
1. 构造 `AppState`:压入足量 block 使**预换行后总逻辑行数 > transcript 视口高度**;其中**至少含一行「会被 `Paragraph` 二次换行」的行**——可靠触发源(任选,建议用前者,免受默认折叠影响):
   - 一个 `TranscriptBlock::Error(..)` 块(必带 ~80 `─` 边框,窄终端下必二次换行);或
   - `tools_expanded = true` + 一个 `output` 含**超 width 长行**的 `Tool` 卡(`visible_tool_output_lines` 未预换行)。
   - 末块为可识别**针标**:`User`/`Assistant` 文本含唯一串(如 `"NEEDLE_LAST_LINE"`)。
2. 渲染到**窄** `TestBackend`(`width` 明显 < 80,如 40;`height` 取使 transcript 视口为中等高度,如 24)。`follows_bottom` 取默认真(贴底)。
3. 提取 buffer 文本(仿 `buffer_to_plain`),**断言**渲染输出**包含**针标串 `"NEEDLE_LAST_LINE"`。
   - **bug 在(`.wrap` 存在)**:超宽行二次换行 → 屏幕行数 > 视口 → 切片末尾针标溢出被裁 → 输出**不含**针标 → **RED**。
   - **修复后(去 `.wrap` + 预换行)**:每逻辑行 1 屏幕行 → 切片 == 视口 → 针标在底部可见 → 输出**含**针标 → **GREEN**。
4. (可选二次断言)transcript 区**非空屏幕行数 ≤ 视口高度**,佐证「无纵向溢出」。

🔴 **红灯停点**:此复现测试是本 change 唯一新增行为锁,首次写成、贴出 red 输出后**停下等确认**,再做修复(去 wrap + 预换行)转 green。

## Risks / Trade-offs

- **去 `.wrap` 后过宽行右端截断** → 接受(Non-Goal)。已应预换行的内容(`User`/`Assistant`、补后的工具输出)与自适配边框不受影响;仅个别宽字符行尾 1 格在窄终端可能被截。
- **复现测试不够「红」**(若选的触发行恰好两套算法一致 → 不二次换行 → bug 不复现 → 测试假绿) → 用「必超宽」的触发源(80 宽边框 / 超 width 长工具输出)而非依赖 emoji/CJK 的 1 格差异;implement 时先确认 red 再修(红灯停点正为此)。
- **既有 insta 快照漂移** → 80 宽默认快照下边框不超宽、去 wrap 通常不变;若某帧含超 80 行才需迁移。implement 跑 `cargo test` 核对,变化帧人工审后更新锁定。
- **窄终端 `Min(8)` 布局**:height 过小会使 transcript 视口被压;测试选合理 height 使视口为正数中等值,避免边界噪声。

## Open Questions(已拍板)

- **Q1**:ADDED「transcript 视口渲染保真」保持不动。
- **Q2**:本期顺带边框按 `width` 自适配(工具卡脚 / Error/Help/Status)。
- **Q3**:复现测试触发源用 `Error` 块(80 宽边框,窄终端必二次换行)。
