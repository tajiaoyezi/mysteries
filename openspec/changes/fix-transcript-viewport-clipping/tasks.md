# Tasks — fix-transcript-viewport-clipping

> TDD:这是 bug 修复,**先写复现测试(red)→ 修(green)**。复现测试为逻辑断言(非纯快照),覆盖「跟随底部时最新内容可见」。
> 🔴 **红灯停点**:复现测试首次写成、贴出 RED 输出后**停下等确认**,再动 `render.rs`(1.2)。

## 0. 实施前确认(开 implement 前)

- [x] 0.1 上游已拍板:Q1=ADDED「transcript 视口渲染保真」保持不动;Q2=本期顺带边框按 `width` 自适配(工具卡脚 / Error/Help/Status);Q3=复现测试触发源用 `Error` 块(80 宽边框,窄终端必二次换行)。design/spec/tasks 已同步。

## 1. 复现测试(强制 TDD · 🔴 红灯停点)

- [x] 1.1 【红】先**只**写复现测试,运行确认失败(RED,原因为「最新内容被裁」非编译错):构造 `AppState` 使预换行后总逻辑行数 > transcript 视口高度,且含一行会触发 `Paragraph` 二次换行的行(建议 `TranscriptBlock::Error(..)` 的 80 宽边框;或 `tools_expanded=true` + 一个 `output` 含超 `width` 长行的 `Tool` 卡);末块为含唯一针标串(如 `NEEDLE_LAST_LINE`)的 `User`/`Assistant`。渲染到窄 `TestBackend`(宽如 40、高使视口为中等正值)、`follows_bottom` 默认真;提取 buffer 文本断言**包含**针标串。
- [x] 1.2 🔴 **红灯停点**:贴出 1.1 测试代码 + RED 输出,**停下等确认**(本 change 唯一新增行为锁首次成型),再写修复。

## 2. 修复(绿,仅 render 层)

- [x] 2.1 【绿】`render_transcript`(~92)去掉 `.wrap(Wrap{trim:false})`;若 `Wrap` import 不再使用则一并清理。
- [x] 2.2 【绿】`visible_tool_output_lines`(~695)/ 工具输出渲染补**长行预换行**:按 `width - display_width("│ ")` 复用 `wrap_text`,使展开态长工具输出换行可读(不整行右端截断丢失)。
- [x] 2.3 核对其余 block 行宽:应换行才可读的(`Notice` 文本等)预换行到 ≤ width;固定 80 宽边框(工具卡脚 / `Error`/`Help`/`Status`)改为按 `width` 自适配生成(`"─".repeat(width.saturating_sub(k))`),占恰好 1 屏幕行、不顶高。
- [x] 2.4 复现测试转 **GREEN**;补 spec scenario 2 的断言(边框按 width 自适配占恰好 1 屏幕行,后续内容不被挤出)与 scenario 3(展开态长工具输出预换行)。

## 3. 回归与验证

- [x] 3.1 `cargo build` 通过;`cargo test` 全绿。
- [x] 3.2 迁移因去 `.wrap` 漂移的 insta 快照(若有;80 宽默认帧通常不变,仅含 >80 宽行的帧可能变),人工审后更新锁定。
- [x] 3.3 `openspec validate fix-transcript-viewport-clipping --strict` 通过。
- [ ] 3.4 TUI 手动冒烟:窄终端下发消息后,最新 `User`/`Assistant` **立即可见**(不再等模型继续输出才「顶上来」);含工具卡 / 长输出的会话滚动正常。
