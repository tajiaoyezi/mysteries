# 2026-06-29 · 34 · archive enable-mouse-wheel-scroll

## 决策

- **启用鼠标捕获,滚轮 → transcript 滚动** | 主导:用户(实测「滚轮误触输入历史」)。根因:`↑↓` 改归历史后,鼠标捕获关闭时 WT alternate-scroll-mode 把**滚轮翻译成 `↑/↓` 方向键** → 误翻历史;滚轮与键盘 `↑↓` 同源不可区分,**唯有捕获鼠标**才能分开
- **trilemma:全屏 TUI 里 {↑↓=历史, 滚轮=滚动, 普通拖=复制} 最多同时占两** | 终端协议硬限:开捕获(得滚轮事件)⊻ 原生普通拖选择,Shift+拖是唯一旁路。用户在 AskUserQuestion 选「开捕获:滚轮滚 + ↑↓历史」(A),复制让位 Shift+拖 | 弃:不捕获(滚轮翻历史)、↑↓还原滚动历史改 Ctrl+↑↓
- **反转「终端原生复制(不捕获鼠标)」requirement** | REMOVED 旧(Reason/Migration)+ ADDED「鼠标滚轮滚动(捕获鼠标)」。`EnableMouseCapture`(setup)+ `DisableMouseCapture`(`restore_terminal`,Drop + panic hook 共用单一路径,异常退出也解除不残留)
- **滚轮 3 行/次,映射抽纯函数** | `mouse_wheel_scroll_action(kind)`:`ScrollUp`/`ScrollDown` → `scroll_up`/`scroll_down` 3 行,其余 kind → None;复用 `apply_scroll` 取 total/viewport。键盘 `↑↓` 走 `Event::Key`、滚轮走 `Event::Mouse`,**事件类型层面天然分离**
- **真机验证为验收门(未完成,task 4.1 留空)** | 启用捕获能否让滚轮以 `Event::Mouse` 到达取决于 WT/ConPTY 转发(历史上「实测不可用」)；**用户尚未真机验证**滚轮是否真滚 / Shift+拖是否能复制。若 ConPTY 仍不转发则滚轮无效(降级:键盘 Page/Home/End 不受损)
- **被 epic D 取代的预期** | 用户后续选定「内联渲染(像 Claude Code)」epic D:弃 alt-screen + 弃捕获,transcript 交终端原生 scrollback + 选择。**D 落地将反转本 change**(回到不捕获);本 change 作为 D 之前的临时方案存档,使滚轮至少能滚(代价 Shift+拖)
- **审查**:独立 cargo/clippy + 读 `terminal.rs`(两条退出路都 `DisableMouseCapture`)+ 滚轮映射 + 事件接线;代码通过,**唯运行时行为(WT 是否转发滚轮)我测不了,交用户**

## 变更

- `src/tui/terminal.rs`:setup `EnterAlternateScreen + EnableMouseCapture`;`restore_terminal` `DisableMouseCapture + LeaveAlternateScreen`
- `src/tui/mod.rs`:`Event::Mouse(me) → handle_mouse_wheel`;`MouseWheelScrollAction` + `mouse_wheel_scroll_action`(3 行)+ `apply_mouse_wheel_scroll` + 映射单测
- `设计规范/02`:滚轮滚动 + 复制改 Shift+拖
- spec:`tui-shell` REMOVED 终端原生复制(不捕获鼠标) + ADDED 鼠标滚轮滚动(捕获鼠标)
- 验证:`cargo test --lib` 357 passed;`clippy` 零警告;`validate --strict` 过(proposal 非标准小节触发 2 条非阻塞 warning)

## 待决

- **真机验证(用户,task 4.1)**:WT 滚轮是否滚 transcript、↑↓ 是否仍历史、Shift+拖是否能复制。失效则反馈回退
- **epic D(内联渲染,像 Claude Code)**:弃 alt-screen + 捕获,终端原生滚动 + 选择,↑↓ 历史三件全有;将 REMOVE/重做本 change + jump-to-bottom + in-app 滚动 + 浮层。下一步设计

## 引用

- change:`enable-mouse-wheel-scroll`(archive `changes/archive/2026-06-29-enable-mouse-wheel-scroll`)
- 关联:`add-input-history-and-permission-modes`(32,↑↓ 改历史是本 bug 的因)
- session 主导:用户实测滚轮误触 → 主 agent 系统调试定位 alternate-scroll-mode + 读 spec「不捕获鼠标」确认架构冲突 → AskUserQuestion 摆 trilemma → 用户选捕获(A)→ 子 agent 实现 → 主 agent 复核(代码通过,真机待用户)→ 用户进一步选 epic D(内联渲染),本 change 降为临时方案
