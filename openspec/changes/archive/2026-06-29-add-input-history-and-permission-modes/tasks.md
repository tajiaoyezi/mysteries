## 1. permission-gate:模式策略(headless 强制 TDD)

- [x] 1.1 [红] 写 `auto_allows(mode, level)` 矩阵测试:`Normal`×{Edit,Execute}=false;`AcceptEdits`×Edit=true、×Execute=false;`Yolo`×{Edit,Execute}=true。**新接口 `PermissionMode` 首次成型 → 贴红灯输出后停下等确认。**
- [x] 1.2 [绿] `PermissionLevel` 细分 `{ReadOnly, Edit, Execute}`;新增 `PermissionMode {Normal, AcceptEdits, Yolo}` + `auto_allows`,最小实现过 1.1。
- [x] 1.3 [绿] 各工具声明类别:`edit.rs` 两工具→`Edit`、`shell.rs`→`Execute`、`fs.rs` 四读类→`ReadOnly`;更新 `gate()` match(非 ReadOnly→decider)与既有 permission/tool 测试、MockTool,全绿。

## 2. decider 接模式(共享句柄)

- [x] 2.1 [红] 写 `ChannelDecider` 持 `Arc<Mutex<PermissionMode>>` 的测试:`Yolo`+`Execute`→`decide()` 返回 `Allow` 且**不**发 oneshot/channel;`AcceptEdits`+`Edit`→`Allow` 不往返;`AcceptEdits`+`Execute`→走 channel(等回送)。**ChannelDecider 新签名 → 贴红灯停点等确认。**
- [x] 2.2 [绿] `ChannelDecider` 构造注入共享模式句柄;`decide()` 先查 `auto_allows` 命中即 `Allow`,否则照旧 oneshot 往返。

## 3. TUI:输入历史 reducer(纯函数单测)

- [x] 3.1 [红] 写历史 reducer 测试:`↑` 逐条回溯、`↓` 前进、`↓` 越过最新恢复草稿、键入字符脱离历史、连续重复提交去重。**reducer 接口首次成型 → 贴红灯停点等确认。**
- [x] 3.2 [绿] `AppState` += `input_history`/`history_cursor`/`draft`;实现 reducer;接入 `on_key` 主输入态 `↑↓`(浮层 handler 已 early-return,天然 gated);提交时 push+去重、游标归草稿;字符/Backspace 输入归草稿态。

## 4. TUI:Shift+Tab 切模式 + 状态行(事后)

- [x] 4.1 BackTab 循环切模式:`on_key` 顶部(先于浮层、同 `ctrl+o` 档)处理 `KeyCode::BackTab`,写共享 `Arc`;`AppState` 持同一句柄。切换序列 `Normal→AcceptEdits→Yolo→Normal` 加纯逻辑单测。
- [x] 4.2 `render.rs` 状态行(C10)追加 `MODE:<mode>` 段,`Yolo` 用强调/warning 色;insta 快照锁定(token 名)。
- [x] 4.3 集成验证:`Yolo` 下 `Execute` 工具不产生 `pending_permission`(C6 不渲染)——单测或快照背书 tui-shell scenario。

## 5. 设计规范 + 校验

- [x] 5.1 `设计规范/03-组件清单.md`:C10 补 `MODE` 段、C11 补 `↑↓` 历史(引 `02-布局与交互.md` 键位)、C6 注「自动放行时不渲染」。
- [x] 5.2 `cargo test`(全绿)+ `cargo clippy --all-targets -- -D warnings`(零警告)+ `openspec validate add-input-history-and-permission-modes --strict`(过)。
