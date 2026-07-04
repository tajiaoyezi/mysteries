# 2026-07-04 · 44 · archive-add-diff-highlight

## 决策

- diff 体**恒显、与 tools_expanded 解耦**(D6):折叠配额 8 屏行 / 展开 24 | 选:按工具类型分型(diff 高价值默认露出,输出噪音默认收起,对齐 Claude Code 形态) | 弃:diff 仅展开可见(原稿,真机证实默认态信息量不足)、单卡独立展开(v1 Non-Goal,状态管理大改) | 主导:用户真机反馈后拍板 A | 依据:真机截图(折叠 diff 体 + 23 条 = 8 + 其余 15,数字吻合)
- 截断按**屏行**预算(非逻辑行) | 弃:按逻辑行计(原稿;对抗审查证明 minified 单行文件下 1 条逻辑行可折出数百屏行,cap 在最需要的场景失效) | 主导:对抗审查边界攻击路 | 依据:code + 单测(中途截断 N=1)
- diff 行内容宽 = `width.saturating_sub(4).max(1)`(`│ ` 2 + 标记 2,区别于 output 的 −2) | 三路审查独立撞到「照抄 −2 溢出 2 列被 Paragraph 静默裁尾」;写死进 spec/design/tasks + 窄视口 CJK 折行快照锁定
- 折叠摘要 ` · +A −D ⌄` 仅 `Done`(显式判态;Error 的 exit 亦 None,仅靠分支位置挡不住)、零侧双向省略、`−` 用 U+2212;diff 体本身不分态恒渲(呈现"请求的变更",计数才暗示已应用)
- 头行 args 无条件 preview 化(`path=...`,整段转义 JSON 由 diff 体取代);有意快照 churn 仅 `tui_tool_card_expanded_done` 一处
- 流程:proposal 先经 4 维对抗审查(代码事实 / 契约质量 / 边界攻击 / 实施可行性),36 findings → 坐实 15 修入(含两处 HIGH 契约矛盾:空 diff 零回归 vs 无条件 preview 互斥、折叠摘要属主 requirement 冲突)、6 条攻击面证实扛得住、零误报;实施经两轮 dispatch(基础 + D6),主 agent 逐行复审 + **5 组变异全杀**(宽度 −4→−2、去 Done 判、屏行→逻辑行、折叠配额 8→24、删折叠 diff 渲染)

## 变更

- `render.rs`:`diff_body_lines(diff, theme, width, max_rows)`(折行 / 屏行截断 / 前缀,纯函数)+ `collapsed_diff_summary_spans`;`tool_card_lines` 折叠分支单行头后、展开分支头行后接 diff 体;`DIFF_MAX_ROWS = 24` / `DIFF_COLLAPSED_MAX_ROWS = 8`
- spec:tui-shell 两条 MODIFIED(「工具卡 C5 渲染」重写含 diff 体 / 宽度 / 双配额截断 / 折叠计数;「工具输出折叠与全局展开」折叠态语义 + 摘要判定链 running → exit → diff 计数 → 行数入 spec,顺带删「结构态(最小色)」历史残句防其随 MODIFIED 写回)
- 测试 503 → 519;新增 7 快照(midnight,daylight 豁免:带色断言按 token 名编码、主题无关);change 前既有快照仅 expanded_done 头行更新
- 独立测试助手 `tool_card_with_args`(共享 `tool_card` 夹具未动——其 path-only args 是既有快照零 churn 的前提)

## 待决

- 权限框(完整、warning_bg)与展开卡(截断、bg.base)同屏重复:v1 接受,不按 call id 抑制
- Error 卡折叠也显红绿 diff 体:统一规则最简,误读风险由计数仅 Done 缓解;真机长用若觉误导再收窄
- minified 单行未真机单验(快照 + 同一截断 machinery 覆盖;真机已验折叠 8+15 截断)
- `diff_body_lines` chunk 循环顶部 cap 检查不可达(尾部检查覆盖所有返回路径),无害防御代码,不值得返工

## 引用

- OpenSpec change:`add-diff-highlight`(propose dbaf59d → 审查修订 d48a1c4 → D6 修订 345306c → feat 2fbff19)→ archive/2026-07-04-add-diff-highlight
- 相关 log:[[2026-07-03-42-archive-add-markdown-render]](渲染线程排定处,本件收官)、[[2026-07-04-43-archive-auto-context-window-and-copy-hint]](同日前件)
- 跨越 session:本会话(propose → 4 维对抗审查 → dispatch 实施 → 主 agent 复审 + 变异 → 两轮真机反馈修订(展开验收、D6 恒显)→ 收口)
