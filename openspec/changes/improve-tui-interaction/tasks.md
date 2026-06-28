# Tasks — improve-tui-interaction

> TDD 分界:**强制红-绿** = headless 纯逻辑(A 折叠态字段+`ctrl+o` 路由 / B 滚动原语+键路由 / C 降级契约逻辑);**事后 insta** = 折叠渲染与快照迁移;**手动** = 真机滚轮核验 / TUI 冒烟。
> 🔴 **红灯停点**(各在测试首次成型、贴红灯输出后**停下等确认**再写绿):① A 折叠状态字段 + `ctrl+o` 键路径(1.2)② B 新滚动原语 + 新滚动键路径(3.2)③ C2 诊断事件日志路径(4.3)。**C 主线不新增 mouse capture 模式/事件路径**(根因在平台、捕获已正确)。

## 0. 实施前确认(开 implement 前,先与上游对齐)

- [x] 0.1 与上游确认 design.md「Open Questions」并同步物料:A1/A2/A3/B1 全锁定照 spec 现状实现;C1 defer(本期不做键盘滚动提示);C2 纳入 env `MYSTERIES_TUI_DEBUG_EVENTS` 诊断开关,日志落 `std::env::temp_dir()` 下固定名,失败静默降级、不阻主循环、禁记凭据。同步后 `openspec validate improve-tui-interaction --strict` 通过。

## 1. A — 折叠态与 ctrl+o 路由(headless 逻辑,强制 TDD)

- [x] 1.1 【红】先只写测,运行确认失败(原因正确非编译错):`AppState` 默认 `tools_expanded == false`;`toggle_tools_expanded` 翻转;`ctrl+o`(`Char('o')`+`CONTROL`,`Press`)经 `on_key` 翻转 `tools_expanded` 且 `input` 不出现 `o`;`Release`/`Repeat` 不翻转。
- [x] 1.2 🔴 **红灯停点①**:贴出 1.1 测试代码 + 失败输出,**停下等确认**(新折叠状态字段 + `ctrl+o` 键路径首次成型),再写绿。
- [x] 1.3 【绿】最小实现:`AppState` 加 `tools_expanded`(默认 false)+ `toggle_tools_expanded`;`on_key` 在 `KeyCode::Char(ch)` arm **之前**拦截 `ctrl+o`。不提前加未被测试要求的功能。
- [x] 1.4 边界(连写不停):`ctrl+o` 在 pending 权限态 / 运行态下只翻折叠、不干扰 Esc 三态与权限 y/n 分流——补测。

## 2. A — 折叠渲染(render,事后 insta)

- [x] 2.1 `render::tool_card_lines` 据 `tools_expanded` 二选一:折叠 = 单行头 + 结果摘要(`· {N} 行 ⌄` / `· exit {code}` / `· 运行中…`);展开 = 现状全量(头/体/脚 + 截断标记)。对照 `设计规范/03` C5(头/体/脚结构)。
- [x] 2.2 事后 insta(对眼):折叠态单行卡 + 展开态全量卡 各一帧带色快照;done / running / error 三态折叠行各锁;首帧人工对 `设计规范/03` C5 审核后锁定。
- [x] 2.3 迁移受影响既有快照(默认折叠后这些帧变化):`tui_tool_card_done` / `tui_timeline_tool_then_final_answer` / `tui_run_shell_exit_foot` / `tui_permission_state`(含 done 卡)——人工对 C5 审核后更新锁定。
- [x] 2.4 「仅折叠 Tool 块」回归:`User` / `Assistant` 折叠态仍全文渲染(带色快照)。

## 3. B — 键盘滚动键补全(headless 逻辑 + 键路由,强制 TDD)

- [x] 3.1 【红】先只写测,运行确认失败:`scroll_to_top` → `scroll_offset == 0` 且 `follows_bottom == false`;`scroll_to_bottom` → `follows_bottom == true` 且 `visible_scroll_offset` 回底;`↑`/`↓` 经 `handle_scroll_key` 映射 `scroll_up`/`scroll_down`(1 行);`Home`/`End` 映射 `scroll_to_top`/`scroll_to_bottom`(均仅 `Press`)。
- [x] 3.2 🔴 **红灯停点②**:贴出 3.1 测试代码 + 失败输出,**停下等确认**(新滚动原语 + 新滚动键路径首次成型),再写绿。
- [x] 3.3 【绿】最小实现:`app.rs` 加 `scroll_to_top` / `scroll_to_bottom`;`mod.rs::handle_scroll_key` 加 `Up`/`Down`/`Home`/`End` 分支(复用 `apply_scroll`,仅 `Press`)。
- [x] 3.4 零回归:既有 `PageUp`/`PageDown` + 鼠标滚轮 + 行级方法 + offset/clamp 测保持绿。

## 4. C — 滚轮降级契约 + 诊断(诚实可达;键盘兜底 = B)

- [x] 4.1 契约测(逻辑):纯键盘(**无任何 `MouseEvent`**)经 `scroll_to_top`/`scroll_to_bottom` 达顶 / 回底;行级键与鼠标滚轮共用同一 `scroll_up`/`scroll_down` 原语(键盘 = 滚轮能力超集)。
- [x] 4.2 `terminal.rs` **不改**:确认鼠标捕获仍开启(根因在 ConPTY 平台、非捕获缺失);**不**新增 mouse capture 模式 / 事件路径(故 C 主线无新红灯)。
- [x] 4.3 诊断:env `MYSTERIES_TUI_DEBUG_EVENTS` 门控,`run_tui` 把经过脱敏的原始 `Event` 摘要落日志;核心 `debug_event_line(&Event) -> String` 为纯函数——先写失败测(🔴 **红灯停点③**:新事件诊断路径首次成型,停下等确认),再绿。失败静默降级、不阻主循环;日志落 `std::env::temp_dir()` 下固定名;**禁记任何凭据**(CLAUDE.md)。
- [ ] 4.4 手动冒烟(真机核验根因,非自动):在 Windows Terminal(记录 `conhost` 版本)下确认是否收到 `Event::Mouse(ScrollUp/Down)`;并验证键盘 `↑↓`/`PgUp·PgDn`/`Home·End` 全覆盖滚动正常(滚轮不转发时键盘兜底无损)。

## 5. 收尾验证

- [x] 5.1 `cargo build` 通过;`cargo test` 全绿(含新红-绿与迁移后的 insta)。
- [x] 5.2 `openspec validate improve-tui-interaction --strict` 通过。
- [ ] 5.3 TUI 手动冒烟:工具卡默认折叠、`ctrl+o` 全局展开 / 折回;`↑↓`/`Home`/`End`/`PageUp`/`PageDown` 键盘全覆盖;滚轮在转发的终端可用、不转发时键盘兜底无损。
