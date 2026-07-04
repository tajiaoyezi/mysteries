# Design — add-diff-highlight

## D1 范围与数据源

diff 体仅 write_file / edit_file(`compute_diff(card.name, card.args)` 非空时);数据源为 **args 纯推导**(write_file → content 全 `Add`;edit_file → old_string 全 `Del` + new_string 全 `Add`),MUST NOT 读文件——沿用 `compute_diff` 既有约定(app.rs 单测 `..._without_reading_files` 锁定)。权限确认框的 diff(warning_bg 风格、不设上限——授权前需完整审阅)不动;权限框 uncapped 与卡片 capped 的风格/截断差异为有意设计。

- **被否**:执行后读盘算真实 diff(行级 LCS)——引入 IO 与时序问题(文件可能已再变),且 str-replace 语义下 old→new 块状呈现已忠实;v1 不做。

## D2 展开态渲染

- **插入位置**:头行之后、output 行之前(output 是工具回执文案,保留)。
- **行结构**:`│ `(`border_subtle`)+ `+ `/`− `/`  `(Ctx 防御,非契约面)标记 + 文本;`Add` → `success.fg`、`Del` → `error.fg`;底色 **`bg.base`**(卡片体风格;权限框用 warning_bg,两处风格独立)。`−` 用 U+2212(与既有 `diff_line` 前缀同字符)。
- **宽度数学(易错点,写死)**:`tool_card_lines` 收到的 `width` 是 transcript 区**整行宽**;diff 行前缀共 **4 列**(`│ ` 2 + 标记 2),内容宽 = `width.saturating_sub(4).max(1)`——**区别于 output 行的 −2**(visible_tool_output_lines,render.rs:875),照抄会溢出 2 列被 Paragraph 静默裁尾、破坏选区复制。续行 `│ ` + 两空格占位(恰补标记列),首行续行内容同宽、同色、不重复标记。折行用既有 `wrap_text`(按 `char_width` 显示宽度,CJK 正确)。
- **截断按屏行(对抗审查修订)**:屏行预算——按折行后显示行计,允许止于某条 `DiffLine` 折行中途;**双配额**(D6 修订):展开 `DIFF_MAX_ROWS = 24`、折叠 `DIFF_COLLAPSED_MAX_ROWS = 8`(均具名常量);尾行 `⋯ 其余 N 行`(N = 未被**完整**显示的 `DiffLine` 数,`text.muted`,带 `│ ` 前缀,循 truncated 行先例)。
  - **被否(原稿)**:按逻辑行计——minified 单行文件(.min.js / base64 / 单行 JSON)只有 1 条逻辑行,窄视口可折出数百屏行,cap 在最需要它的内容形状上完全失效,且行集每帧全量重建、成本每滚动一次付一次。
  - 观感注记:edit_file 的 Del 全在 Add 前,old 超预算时 Add 会整体不可见——由折叠摘要 `+A −D` 与尾行计数补偿,v1 接受。
- **头行 args**:edit/write 展开态改用 `tool_args_preview`(`path=...`),**无条件**(空 diff 卡亦 preview 化——这正是点名 churn `tui_tool_card_expanded_done` 的来源);preview 缺 path 时沿用既有整段 JSON fallback。其他工具头行不变。

## D3 折叠态摘要

`Done` 且 diff 非空 → ` · +A −D ⌄`(`+A` success.fg、`−D` error.fg,` · ` 与 ` ⌄` 用 `text.secondary`(循既有摘要整段色),零侧双向省略:write 全 Add → ` · +12 ⌄`,edit new 空 → ` · −2 ⌄`),判定链(已提入 spec):`running` → `exit` → **diff 计数** → 行数;新分支 MUST **显式判 `Done`**——`Error` 的 exit 亦 `None`,仅靠插入位置挡不住。

- `Running` 维持 ` · 运行中…`;`Error` 维持既有(落 output 行数分支,如 ` · 2 行 ⌄`——base spec 原无 Error 摘要定义,按代码现状锚定)。展开态 diff 体**不分态**渲染(Running/Error 亦渲,呈现"请求的变更";Running 时 args 已完整——两条流式路径均攒完再 `from_str`,`complete()` 返回后才发事件,零闪变)。
- 同屏重复认领:`WaitingForPermission` + `tools_expanded` 时,权限框(完整 diff)与卡片(capped diff)同屏重复,展开是显式全局态,v1 接受,不按 call id 抑制(过度设计)。

## D4 实现形态

- 拆纯函数:`diff_body_lines(diff: &[DiffLine], theme, width) -> Vec<Line<'static>>`(折行 / 截断 / 前缀在此,cap 与 wrap 可直接单测),`tool_card_lines` 展开分支以 `compute_diff` 结果调用;空 diff → 空 Vec(其他工具零侵入)。
- 行数一致性:transcript 高度/滚动计数与渲染共用 `tool_card_lines` 单一来源(`transcript_line_count` → `transcript_content_lines`,render.rs:59-61),MUST NOT 另写第二套行数计算。
- 零外溢锚点:`compute_diff` 的权限框两调用点(render.rs:153 高度、:936 内容)与 `diff_line`(:990,唯一调用 :938)均不改名不改语义。

## D5 快照策略

- **有意 churn 仅一处**:`tui_tool_card_expanded_done`(write_file、args 仅 path → 头行 preview 化;该卡无 content,diff 体为空,其余行不变)。
- **新增快照**(midnight):edit_file 展开 diff 体、write_file 展开全 Add、折叠 `+A −D`(含双向省略)、短行超限截断、单条超长行屏行截断、窄视口 CJK 折行、Running/Error 展开不分态。
- **不做 daylight 版**:沿工具卡快照系列既有 midnight 单版惯例;带色断言按 token 名编码(`buffer_to_styled` 记 `success.fg` 等语义名而非 RGB),主题无关;daylight 色板回归由既有 permission / markdown daylight 快照承担。
- **churn 判据(可核清单)**:跑完全部测试后 `git status --short` 预期 = 恰 1 个修改的 .snap(`tui_tool_card_expanded_done`)+ 新增 .snap 若干,无其他修改。
- 夹具纪律:既有快照零 churn 依赖共享 `tool_card` 助手(render.rs:2086)args 恰好无 content/old/new——**MUST NOT 修改该助手签名与默认 args**,新 scenario 内联构造或另设助手。

## D6 diff 恒显、与 tools_expanded 解耦(真机反馈修订)

真机反馈:全局折叠(默认)下 diff 完全不可见,全局展开则所有工具输出一起爆炸——全局二元 toggle 一刀切。对齐 Claude Code 的**按工具类型分型**策略:diff 是高价值内容默认露出,输出噪音默认收起。

- diff 体与 `tools_expanded` 解耦、**恒显**:折叠态渲于单行头之后(仍无 output / 脚 / 边框),配额 8 屏行;展开态维持头行与 output 之间,配额 24。read / run_shell 等其他工具折叠行为零变化(这正是分型的意义)。
- 折叠 diff 行保留 `│ ` 前缀:实现零分叉(同一 `diff_body_lines`)、视觉上与卡片成组;`⌄` 语义仍成立(展开看 output 与更长 diff)。
- 恒显**不分态**(含 `Error` 折叠卡):统一规则最简;误读"已应用"的风险由折叠计数仅 `Done` 缓解(计数才暗示结果,diff 体呈现请求)。若真机觉得 Error 卡红绿误导,后续再收窄。
- **被否**:维持 diff 仅展开可见(方案 B,默认态信息量不足,真机已证);单卡独立展开(方案 C,v1 Non-Goal,状态管理大改)。
- 快照影响:`tui_tool_card_collapsed_diff_counts`(本 change 内新增)将更新以体现折叠 diff 体与 8 行截断;**change 前既有快照零 churn 结论不变**(既有折叠快照中的 write/edit 卡均 path-only、diff 恒空)。

## 测试断点

- 单测(`diff_body_lines` 纯函数):屏行 cap 边界(预算恰满 / 超 1 行 / 单条超长行中途截断)、续行前缀与不重复标记、内容宽 −4、空 diff 空 Vec、折叠计数双向省略、计数分支仅 `Done`。
- 快照:D5 清单 + churn 核对。
- 门禁:`cargo test --lib` 全绿、`cargo clippy --all-targets -- -D warnings` 零警告、`openspec validate add-diff-highlight --strict`。
