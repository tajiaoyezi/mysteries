# 2026-06-28 · 16 · archive improve-tui-interaction

## 决策
- 工具卡折叠:默认折叠单行(glyph+名+args 摘要 + 结果摘要 · N 行 ⌄ / · exit / · 运行中…)、ctrl+o 全局 toggle | 选:A1 单行带 ⌄ affordance、A2 仅全局、A3 ctrl+o | 弃:1~2 行预览(高度不定)、纯单行无提示(ctrl+o 不可发现)、单条展开(需 transcript 焦点模型,成本高) | 主导:用户拍板(6 OQ 全照推荐锁定) | 依据:design ① / 设计规范 C5 / 对眼快照
- 折叠只作用 Tool 块,User/Assistant 全文 | 主导:讨论收敛(护 15 刚修的「最终回答钉底可见」) | 依据:design ①.c / 快照 tui_folding_only_affects_tool_blocks
- 键盘滚动全覆盖:↑↓ 行级 / Home 到顶 / End 回底恢复跟随,与 PageUp/PageDown + 滚轮共用 scroll_up/scroll_down 原语(键盘 = 滚轮能力超集) | 选:↑↓ 判 transcript 滚动 | 弃:↑↓ 判输入历史(本期无;记账:将来加按语境回收键位) | 主导:用户拍板 B1 | 依据:design ② / 契约测 repeated_line_keys_match_mouse
- 滚轮无响应根因 = ConPTY/Windows 构建平台限制(非配置错、非 crossterm 缺陷) | 选:C1 接受限制+键盘兜底(主线)+ C2 诊断仪表(配套) | 弃:C3 自写 Win32 ReadConsoleInputW(旧 ConPTY 不往 input buffer 塞鼠标记录,绕了也收不到) | 主导:sub-agent 调研收敛 + 用户拍板(C2 纳入 / C1 提示 defer) | 依据:design ③ / microsoft·terminal #376·#545
- 诊断脱敏:debug_event_line 显式 match 全 Event 变体,Char→<redacted>、Paste→只 len,禁记 prompt 正文;env MYSTERIES_TUI_DEBUG_EVENTS 门控、写盘失败静默降级 | 主导:主 agent 红灯③打回(原测只锁 Char,漏 Paste 这一粘贴泄露最大面) | 依据:spec 诊断 requirement

## 变更
- src/tui/{app,mod,render}.rs:折叠态+ctrl+o 拦截 / scroll_to_top·bottom + ↑↓·Home·End 路由 / 折叠渲染二选一 / debug_event_line + run_tui 诊断接线;迁移 7 快照 + 新增 2(折叠·展开)
- spec tui-shell:ADDED 折叠、键盘全覆盖+滚轮降级、诊断事件日志;MODIFIED transcript 滚动(+↑↓/Home/End + scroll_to_top/bottom)
- 验证:cargo test 全 target(176 lib + 1 e2e)、clippy --all-targets 零警告、fmt 净、validate --strict 过
- 流程:propose 一次过(无打回);3 红灯停点——①折叠(打回补 tools_expanded 翻转防退化)②滚动(一次过)③诊断(打回补 Paste 脱敏)

## 待决
- TUI 真机冒烟(tasks 4.4/5.3):Windows Terminal 滚轮实测 + ctrl+o/↑↓/Home·End 留用户跑
- C1 滚动可发现性提示:defer,冒烟看滚轮实况后再定是否加 / 加在哪
- 承自 15 未动:强制收尾 complete 前未 emit CallingModel 的状态栏小瑕疵;git 身份 wanglei30 临时、旧 leafiellune 在 refs/original+reflog 待 purge
- (target-codex/ 已 GONE,本会话核实,不再列)

## 引用
- change:improve-tui-interaction(design ①–③ 全量 rationale;archive 路径 changes/archive/2026-06-28-improve-tui-interaction)
- 调研:microsoft/terminal #376·#545(ConPTY mouse)、PowerShell/Win32-OpenSSH #1863(conhost 10.0.22523 修复)、MS Learn VT input mouse
- 前置:claude-code-style-loop(15);6 OQ 拍板:A1/A2/A3/B1 锁、C1 defer、C2 纳入
- session:主 agent 编排——propose 审查一次过 + 6 OQ 拍板 + 3 红灯停点(①③ 打回纠正)+ 全量终审 + 对眼快照
