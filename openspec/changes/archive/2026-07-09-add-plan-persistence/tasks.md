## 1. 编译骨架(不计红绿:类型/签名先落地,让行为测试可编译)

> 说明:纯 serde derive / 新变体 / 新签名在 Rust 里无「运行期红灯」态——要么缺失即编译错、要么加了即绿,没有「能编译但断言失败」的中间态。故按 CLAUDE.md「红灯须失败原因正确、**非编译错**」,把类型引入作为**不计红绿**的骨架先落,真正的红-绿留给 §2 的行为。

- [x] 1.1 `StepStatus`(`src/tool/plan.rs`)加 `#[derive(Serialize, Deserialize)]`(additive,**保留既有 `Copy` 等**)。
- [x] 1.2 `ActivePlan` / `ActiveStep`(`src/tui/app.rs`)加 `#[derive(Serialize, Deserialize)]`。
- [x] 1.3 `SessionLine`(`src/session/mod.rs`)加 `Plan(ActivePlan)` 变体。
- [x] 1.4 `write` 签名加 `plan: Option<&ActivePlan>` 参(**骨架期先忽略该参、不写 `Plan` 行**)。
- [x] 1.5 `load` 返回 `(SessionMeta, Vec<Message>, Vec<TranscriptBlock>, Option<ActivePlan>)`(**骨架期第 4 位恒 `None`、不路由 `Plan` 行**);**`load` 自身 match(`session/mod.rs:88-96`)与 `read_session_summary` 的 match(`174-183`)均无通配 `_` 臂**,须各加 `SessionLine::Plan(_) => {}` 补 E0004 穷尽性(骨架期 `load` 的 `Plan` 臂先不路由、`read_session_summary` 恒忽略)。
- [x] 1.6 编译器驱动改全调用点。**先满足 `cargo build`(仅编译生产代码)**——三个 **生产** 解构/调用点必改:`tui/mod.rs:337`(picker hot-swap 的 `load` 3 元组解构)、`379`(`--continue` 的 `load` 解构)、`424`(autosave `write` 3 参)。**再满足 `cargo test` / `clippy --all-targets`**——测试侧 `load` 解构点(`session/mod.rs:373` / `396` / `573` / `591` / `705`)补第 4 元素、`write` 调用点(`session/mod.rs:314` / `372` / `568` / `685` / `686` / `704` 与 tui 测试 `1676` / `1702`)补参。此步不计红绿(骨架就位、行为尚未实现)。

## 2. 行为红灯 → 绿(强制 TDD,红灯独立成步)

- [x] 2.1 【红】写 session **行为**失败测试(§1 骨架已就位、能编译,以下运行期 assert 失败):
  - ① `write(plan=Some(A))` → `load` 第 4 元素应 == `A`(骨架恒 `None` → 失败)
  - ② 先 `write(Some(A))` 再 `write(Some(B))` 同一会话 → 文件仅一条 `Plan` 行且为 `B`(骨架不写 `Plan` 行 → 失败)
  - ③ 手工构造两条 `Plan` 行 → `load` 应 `Err`(骨架忽略 `Plan`、不 `Err` → 失败)
  - (附:无 `Plan` 行旧 session → `None`、`ActivePlan` / `StepStatus` serde round-trip、**`list_sessions` 枚举含 `Plan` 行的会话 → 摘要正常 / `first_user` 不被 `Plan` 行污染 / 不报错**、**既有「行序无关」测试加一条 `Plan` 行仍正确归位** —— 这些骨架期已满足,作**兼容 / 序列化 / 摘要守护**一并写入,预期直接绿——非红灯项)
- [x] 2.2 停点:§2.1 ①②③ 是 session 内核**行为**红灯首次成型——贴**运行期失败输出**(非编译错)等用户确认后再进 §2.3(遵 CLAUDE.md 折中档红灯停点)。
- [x] 2.3 【绿】最小实现:`write` 的 `Some` 分支追加一条 `Plan` 行;`load` 把 `Plan` 行路由进第 4 元素、**多于一条 → `Err`**(仿 `Meta` 重复);§2.1 全绿。

## 3. TUI 接线:autosave 落 plan + resume/continue 还原(plan-only seam)

- [x] 3.1 autosave:`write_session_snapshot`(`src/tui/mod.rs:415-424`,已持 `state: &AppState`)把 `store.write(..)` 传入 `state.current_plan.as_ref()`。
- [x] 3.2 抽**纯 sync seam** `apply_loaded_plan(state: &mut AppState, plan: Option<ActivePlan>)`,函数体**即 `state.current_plan = plan;` 一句**(`current_plan` 是 `AppState` 普通字段 `app.rs:476`,同步可写)。picker hot-swap 的 `agent_history.lock().await` / `input_tx.send(SetProvider)` / `session_meta = meta`(run_tui 局部)/ `transcript` / provider·model **全部留在各自调用点原地、不进 seam**——它们涉 async / `input_tx` / 外层局部,纯函数覆盖不了,且 `--continue` 的 `history`/`transcript` 启动期已被 move 消耗、供不出「统一 hot-swap seam」的入参。两路唯一真正共享的就是 `current_plan = plan` 这一句。
- [x] 3.3 两路都**必经**该 seam 落 `current_plan`:
  - ① `--resume` picker hot-swap(`mod.rs:335-357`):在既有逻辑**末尾**加 `apply_loaded_plan(&mut state, plan)`(`plan` 取自 `load` 第 4 元素)。
  - ② `--continue`:`SessionStartup`(`mod.rs:84-89`)加 `plan: Option<ActivePlan>` 字段;`prepare_session_startup` 的 Continue 分支(379-388)填 `load` 第 4 元素、Fresh 分支(399-404)填 `None`;`run_tui` 构造 `AppState` 后(`mod.rs:168` 附近)调 `apply_loaded_plan(&mut state, session_startup.plan)`。
- [x] 3.4 【状态断言 · headless 可测 · 纯 sync `#[test]`】对 `apply_loaded_plan` 直接断言:传 `plan=Some(_)` → `state.current_plan == Some(_)`;传 `None` → `current_plan == None`(不建空面板)。**这是「是否真还原」的唯一实质守护**——编译器只逼 `load` 调用点补绑第 4 元素、不逼其被使用(`_`-drop 会静默丢弃 plan、clippy 不报错),两路都经 seam 才有守护。
- [x] 3.5 【状态断言】`prepare_session_startup(Continue)` 单测:曾落盘 plan 的会话 → 返回的 `SessionStartup.plan == Some(_)`(补现有 continue 测试未断言 plan 的缺口)。
- [x] 3.6 【事后 insta · 视觉复用】恢复态面板视觉锁定**复用既有快照**(`tui_active_plan_folded` 折叠态 / in-progress 完整态 / 长文截断态);至多补 1 条「经 seam 还原后渲染 == 既有折叠快照」对照,**不新增独立快照 task**(恢复态与手搓态渲染字节相同)。**设计规范对照点**:该面板晚于 `设计规范/` 冻结日引入、未收录,视觉契约以 `tui-shell` spec「PlanProgress」+ 既有快照为准;配色无改动 → 不需 theme.rs token 单测。

## 4. 收尾验证

- [x] 4.1 `cargo test` 全绿(§2 行为红灯转绿 + 兼容 / serde / 摘要守护 + §3 seam 状态断言 + 既有全量回归)。
- [x] 4.2 `cargo clippy --all-targets -- -D warnings` 零警告(注意:clippy **不抓** `_`-drop,§3.4 状态断言才是接线守护)。
- [x] 4.3 `cargo fmt --all`。
- [x] 4.4 CHANGELOG `[Unreleased]` 补:新增 plan 进度持久化 + `--resume` / `--continue` 恢复;点明**降级不兼容**(升级后写出的含 `Plan` 行会话,回退到旧 v1.1.0 二进制读取会 `Err`)。

## 备注(UI 规则适用性)

- 本 change **不新增 UI 组件**(复用既有 PlanProgress 面板),故「首个 UI 组件快照须人工对 `设计规范/原型截图/` 审一次」不适用;人工审收敛为「恢复态经 seam 渲染与既有面板快照一致性确认」。
