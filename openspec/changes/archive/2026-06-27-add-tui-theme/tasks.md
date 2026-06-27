## 1. theme.rs + token 单测(単測)

- [x] 1.1 【红】写 token 値単測:断言 `Theme::midnight()` 与 `Theme::daylight()` 每个语义 token 的 `Color::Rgb` = `设计规范/01` 表值(如 Midnight `accent.primary == Rgb(0xb1,0x8c,0xf0)`、Daylight `bg.base == Rgb(0xf4,0xf1,0xea)`);确认失败
- [x] 1.2 【绿】实现 `src/tui/theme.rs`:`Theme`(扁平命名色场,见 design D1)+ `midnight()` / `daylight()`(填 01 hex)
- [x] 1.3 【重构】清理;`lib.rs`/`tui::mod` 导出 `theme`

## 2. render 吃 &Theme + 语义上色

- [x] 2.1 【绿】`render(frame, state, &Theme)`:逐处硬编码色(`Cyan`/`Yellow`/`Red`/`DarkGray`)→ 语义 token(品牌/占位 `text.muted`、marker/tag/工具名 `accent.primary`、权限框 `warning.*`、`✓ success.fg`/`✗ error.fg`、phase 按 `02`,见 design D3)
- [x] 2.2 更新 3 调用点:`render.rs` 测试 helper + `mod.rs` run_tui 两处 draw 传 `&theme`;run_tui 内置 `Theme::midnight()`;`cargo build` 通过

## 3. buffer_to_styled 带色快照基建

- [x] 3.1 【绿】实现 `Theme::token_name(color) -> Option<&'static str>`(按字段顺序首个匹配,确定性)+ `buffer_to_styled(buffer, &Theme)`(走 cell:`symbol` + `fg`/`bg`(+关键 `Modifier`)反查 token 名,按 styled-run 分组注入注解,见 design D4);退役 `buffer_to_text`

## 4. 迁移既有 6 快照 → styled(insta 重核)

- [x] 4.1 既有 render 测试改用 `buffer_to_styled`(传 Midnight);重生成 6 个 `.snap`(welcome / permission / phase_status_lines / tool_card running·done·error);`cargo insta review` 逐帧重核(确认仅「加色注解 + 正确 token 赋值」,结构不变,见 design D5)

## 5. Daylight 渲染 + welcome themed 快照 ×2

- [x] 5.1 【绿/insta】加 Daylight 渲染测试:welcome 态分别以 `Theme::midnight()` / `Theme::daylight()` 渲 → 两份带色快照(文本结构同、配色异);确认两帧可分辨

## 6. 对眼 + 收尾(insta · welcome themed 对眼停点)

- [x] 6.1 【insta · 停点】`cargo insta review`:**welcome 两主题首帧人工对 `原型截图/midnight-01-欢迎态` 与 `daylight-01-欢迎态`**(连配色一起核,config.yaml + `设计规范/README` 关卡)。**贴两帧渲染给用户审**
- [x] 6.2 收尾:`cargo build`、`cargo test` 默认全绿(token 単測 + 带色 insta,无终端)、`cargo fmt`;自检:`tui-shell` ADDED(主题令牌 / themed 渲染 / 带色锁)requirements 全有落点(単測 / insta / 对眼 已分类);偏离已标注(256/16 降级 → 1.4、运行时切换 / config 字段延后、diff/C7/滚动/spinner → cut2b-b);**不动 config-layering** 已守
