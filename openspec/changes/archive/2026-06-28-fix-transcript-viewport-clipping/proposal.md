## Why

TUI 发消息后,最新的 `User` / `Assistant` 内容看不到——被遮挡在 transcript 视口**下方**;要等模型继续输出、内容增多把它「顶上去」后才可见。这违背 `设计规范/02-布局与交互`「transcript 自动滚到底、最新内容可见」的核心契约,严重影响基本可用性(用户发完消息看不到自己发了什么、也看不到回答的开头)。

**根因**:transcript 渲染存在**双重换行**,导致「切出的逻辑行数 ≠ 实际屏幕行数」。`render.rs::render_transcript`(~92)给 `Paragraph` 叠了 `.wrap(Wrap{trim:false})`,而 `visible_transcript_lines`(~109)**已**按「预换行后的行数」精确切出 `viewport_lines` 行(`skip(offset).take(viewport_lines)`)。两层换行的宽度算法不一致:预换行 `wrap_text`(~307)用项目自定义 `char_width`(~868),`Paragraph.wrap` 用 ratatui 内部 unicode-width;任何字符两者差 1 格(emoji/CJK/符号),或**未预换行**的长行(`visible_tool_output_lines` ~695 根本没换行、工具卡头/固定 80 宽边框在窄终端),都会被 `Paragraph` 二次换行 → 实际屏幕行数 > 视口高度 → `Paragraph` 从顶部填、底部溢出被裁 → 最新内容(切片末尾)落到视口下方看不见;offset 抬高后才「顶上来」。这是**既有 bug**,非最近的折叠/滚动 change 引入。

## What Changes

- **消除双重换行,让「切出的行数 == 实际屏幕行数」**(上游已拍板:方案 1,**不加依赖**):
  1. 去掉 `render_transcript` 的 `.wrap(Wrap)` —— 预换行成为唯一换行来源,每个逻辑行恰好占 1 屏幕行(过宽则右端**截断**而非换行顶高)。
  2. 确保所有 transcript 行预换行到显示宽度 ≤ 视口宽度:`message_lines`(~267)已 wrap(`User`/`Assistant`);**补** `visible_tool_output_lines` 长工具输出预换行;核对工具卡头 / `Help`/`Status`/`Error`/`Notice`/欢迎态等所有 block 行宽。固定 80 宽边框在更窄终端**接受右端截断**(不顶高,不破坏视口不变量)。
- **锁定回归不变量(spec + 复现测试)**:transcript 视口渲染时**可见屏幕行数 MUST 等于视口高度**;`follows_bottom` 时**最新(底部)内容 MUST 在视口内可见**;所有 transcript 行 MUST 预换行至 ≤ 视口宽度、**不依赖 `Paragraph` 二次换行裁切**。配可复现测试(窄宽 + 超视口多块内容 → 断言最后一个块内容出现在渲染输出里),bug 在时 red、修复后 green。

## Capabilities

### New Capabilities
<!-- 无新增 capability:对既有 tui-shell capability 追加一条渲染保真 requirement。 -->

### Modified Capabilities
- `tui-shell`:**ADDED**「transcript 视口渲染保真(可见行数对齐视口高度 · 底部可见)」—— 现有 spec 有「transcript 滚动」(offset/跟随逻辑,正确,非 bug 源)与「终端文本排版与宽度度量」(逐块 `User`/`Assistant` 换行),但**无**任何 requirement 约束「渲染屏幕行数 == 视口高度 / 不双重换行」这一**新不变量**;故 ADDED 一条,而非 MODIFIED 既有(详见 design.md「决策:spec 挂载点」)。

## Impact

- **code**(本轮 propose 不改,仅登记 implement 触及面):
  - `src/tui/render.rs`:① `render_transcript`(~92)去 `.wrap(Wrap)`(并清理不再需要的 `Wrap` import);② `visible_tool_output_lines`(~695)/ 工具输出渲染补长行预换行(按 `width - "│ "` 宽度,沿用 `wrap_text`);③ 核对工具卡头 / `Help`/`Status`/`Error`/`Notice`/欢迎态行宽(过宽接受右端截断,边框宽度自适配列为 Non-Goal)。
  - **仅 render 层**:`app.rs` 的 `scroll_offset` / `follows_bottom` / `visible_scroll_offset` 等**不改**(offset 逻辑本就正确);channel / agent-task / headless 全不受影响。
- **测试**:新增**复现测试**(逻辑断言,非纯快照):窄 `TestBackend` + 超视口多块(含触发二次换行的行 / 长工具输出)→ 断言最后一个块(最新 `User`/`Assistant`)内容出现在渲染输出里。受影响的既有 insta 快照(去 `.wrap` 后窄宽帧或含超宽行的帧)按需迁移;80 宽默认快照通常不受影响(80 宽下边框不超宽)。
- **设计规范引用 + port/adapt/drop**:本 change = **port(保真修复)**——恢复 `设计规范/02`「transcript 自动滚到底、最新可见」既有契约,无新增/变更视觉语义,无 adapt/drop。
- **deps**:零新增。**明确不引入** `unicode-width`(取舍见 design.md):残留个别宽字符 / 固定边框行尾 1 格右端截断本期**接受**(Non-Goal),不再二次换行顶高。
