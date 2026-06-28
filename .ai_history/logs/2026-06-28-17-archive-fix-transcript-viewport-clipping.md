# 2026-06-28 · 17 · archive fix-transcript-viewport-clipping

## 决策
- transcript 视口遮挡 bug 根因 = 双重换行:render_transcript 在 visible_transcript_lines 已精确切片(skip/take viewport_lines)之上又叠 Paragraph.wrap,与预换行(自定义 char_width)算法不一;未预换行的行(工具输出 / 80 宽边框)被二次换行 → 屏幕行数 > 视口 → 最新内容溢出底部被裁 | 主导:用户报现象 + 主 agent 读码定位 | 依据:code / 复现测试
- 修法 = 去 Paragraph .wrap(预换行为唯一换行来源,每逻辑行恰 1 屏幕行,切片 == 屏幕行) | 选:方案 1 去 wrap + 全量预换行 | 弃:加 unicode-width 校准(架构问题非「宽度算不准」——即便对齐也修不了未预换行的行;且违「不擅自扩 dep」) | 主导:用户拍板方案 1 | 依据:design ②
- 边框 width 自适配(Q2):固定 80 宽边框改 block_top/bottom_border 按 width 生成(width=80 铺满、窄终端不缺角不顶高) | 选:顺带做自适配 | 弃:仅修 bug 接受右端截断 | 主导:用户拍板 Q2 | 依据:design ③
- 残留宽字符行尾 1 格右端截断 = Non-Goal 接受(去 wrap 后从「纵向溢出裁底·功能 bug」降级为「横向 1 格·观感」) | 主导:讨论收敛 | 依据:design ②
- spec 挂载 = ADDED「transcript 视口渲染保真」(Q1) | 弃:并入「终端文本排版与宽度度量」MODIFIED(逐块文本语义,不含跨块视口不变量) | 主导:用户拍板 Q1 | 依据:design ①

## 变更
- src/tui/render.rs:render_transcript 去 .wrap + 清 Wrap import;新增 block_top_border/block_bottom_border 边框自适配,Help/Status/Error/工具卡底用之;visible_tool_output_lines(+width)/Notice/Error 正文补 wrap_text 预换行;collapsed_tool_summary「N 行」改用预换行后行数
- spec tui-shell:ADDED「transcript 视口渲染保真」+ 3 scenario(底部可见 / 边框自适配占 1 行 / 工具输出预换行)
- 测试:复现(逻辑断言:窄终端 + Error 80 边框触发 → 末块针标可见)+ scenario2(边框铺满 width)+ scenario3(工具输出预换行);迁移 5 张含边框快照(fatal_error×2 / help / status / tool_card_expanded_done)
- 验证:cargo test 全 target(179 lib + 1 e2e)、clippy --all-targets 零警告、fmt 净、validate --strict 过
- 流程:propose 一次过 + 1 红灯停点(复现测试,一次过,复用既有 transcript_viewport_height/transcript_line_count 无造假)

## 待决
- 窄终端 TUI 真机冒烟(tasks 3.4):发消息后最新内容立即可见,留用户
- 残留宽字符行尾 1 格右端截断(Non-Goal;边框缺角已由 width 自适配解掉)
- 承自 16 未动:C1 滚动可发现性提示 defer;强制收尾 complete 前未 emit CallingModel 小瑕疵;git 身份 wanglei30 临时、旧 leafiellune 在 refs/original+reflog 待 purge

## 引用
- change:fix-transcript-viewport-clipping(design ①–④ rationale;archive 路径 changes/archive/2026-06-28-fix-transcript-viewport-clipping)
- 前置:improve-tui-interaction(16);3 OQ 拍板:Q1 ADDED、Q2 边框自适配、Q3 Error 块触发源
- session:用户报遮挡现象 → 主 agent 读码定位双重换行根因 → propose 一次过 + 3 OQ(Q2 边框自适配范围用户拍)+ 复现测试红灯(一次过)+ 全量终审 + 对眼 5 迁移快照
