## Context

cut1/cut2a(已 archived)渲出 TUI 四区 + 工具卡 + 全 phase,但用**硬编码 ANSI 色**(`render.rs` 里 `Color::Cyan`/`Yellow`/`Red`/`DarkGray`),快照 `buffer_to_text` **只取 `symbol`、不含色** —— 配色漂移测不出,离 `设计规范/01` Midnight/Daylight 语义色板差一层。本 change = TUI cut2b 的 **a 半**:全主题 + 带色快照基建。用户已确认 styled 快照用 **token 名** 反查、主题选择**不动 config-layering**。

现状(real code):`render(frame, state)` 3 调用点(`render.rs:219` 测试、`mod.rs:44/72` run_tui);`buffer_to_styled` 待建(替 `buffer_to_text`);6 个 text-only `.snap`。视觉权威:`theme.rs`/insta(本 change 落点) > `设计规范/01·02·03` > 原型 > 推断。

## Goals / Non-Goals

**Goals:**

- `theme.rs`:`Theme` + Midnight/Daylight(01 token 值)+ **token単測**。
- `render(.., &Theme)` 语义上色,替硬编码;run_tui 默认 Midnight。
- `buffer_to_styled` 反查 token 名 → 带色快照;迁移既有 6 快照。
- Daylight 渲染 + welcome themed 快照 ×2 + 对眼。

**Non-Goals(留 cut2b-b / 后续):**

- C6 diff body、C7 致命框、transcript 滚动、spinner。
- 256/16 色降级(1.4)、运行时主题切换(config 字段 / `/theme`)。

## Decisions

- **D1 `Theme` = 扁平命名色场,两套构造器。** `pub struct Theme { bg_base, bg_sunken, bg_surface, bg_surface_alt, border_subtle, border_strong, text_title, text_primary, text_body, text_secondary, text_muted, accent_primary, success_fg, warning_fg, warning_bg, error_fg, error_bg, error_border, info_fg: Color }`(字段对 `设计规范/01` 表);`Theme::midnight()` / `Theme::daylight()` 填 `Color::Rgb`。**理由**:扁平字段直观、token単測逐项断言;无需宏 / map。

- **D2 token単測锁值。** 逐 token 断言两套调色板的 `Rgb`(漂移红)。**这是「带色 insta 用 token 名」成立的前提**:token 名映射本身不抓值漂移,值漂移由本単測兜底(见 D4)。

- **D3 `render(frame, state, &Theme)`,更新 3 调用点。** 签名加 `&Theme`,逐处硬编码色 → 语义 token(品牌/占位 `text.muted`、marker/tag/工具名 `accent.primary`、权限框 `warning.*`、`✓ success.fg`/`✗ error.fg`、phase 按 `02`)。`render.rs` 测试 helper + `mod.rs` run_tui 两处 draw 改传 `&theme`;run_tui 内置 `Theme::midnight()`(选择延后)。**render 仍纯函数式吃状态 + 主题,不内置 IO**。

- **D4 `buffer_to_styled(buffer, &Theme)` 反查 token 名。** 走 cell:取 `symbol` + `fg`/`bg`(+ 关键 `Modifier`),`Theme::token_name(color) -> Option<&'static str>`(按字段顺序首个匹配,确定性;值相同的 token 取首名,文档标注)反查;按 styled-run(连续同样式)分组,注入注解(如 `‹accent.primary›…‹/›`)。**两层锁色**:token 名抓**赋值错误**;token **值漂移**由 D2 単測兜底。**理由**:token 名可读、语义、与 01 对齐;原始 Rgb hex 噪声大且脱钩语义(用户已否决)。

- **D5 既有 6 快照迁移 styled。** styled 是 text 的 superset(文本 + 色注解);6 个 `.snap`(welcome/permission/phase/tool_card×3)重生成、`cargo insta review` 一次性重核(并入对眼)。`buffer_to_text` 退役(无残留双表示)。**理由**:一帧一快照、带色;避免 text + styled 双份冗余。

- **D6 Daylight 经测试传入,不引 config 字段。** 两套主题都 `theme.rs` 定义 + 単測 + 快照;Daylight 渲染靠测试 `render(.., &Theme::daylight())`,**无需 config 选择字段** → **不 MODIFY `config-layering`**。运行时切换留后续(config `theme` 小 additive 字段 / `/theme` 命令)。**理由**:本 change 范围 = 定义+测试+锁两套主题;运行时选择是独立 UX,延后避免碰 archived config。

- **D7 测试分界。** token 值 = **単測**(纯值,两调色板);themed render / styled 快照 = **带色 insta**(事后);welcome themed(midnight+daylight)= **人工对眼**(config.yaml + `设计规范/README` 关卡)。既有 render 测试随 D3 签名更新、随 D5 迁移重核。

## Risks / Trade-offs

- **[token 名反查的值歧义]** 两 token 同 Rgb(如 Daylight 某些文字色重合)→ 反查取首名 → 快照可能把 B 标成 A → 缓解:D4 字段顺序确定 + 文档标注;**值的唯一真相在 D2 単測**,反查只为「赋值位置」可读化,歧义不影响值锁。
- **[改 render 签名牵动 3 调用点 + 6 快照]** → 缓解:调用点少且 tui-内部(非跨 capability);迁移走一次 `insta review`;`cargo test` 全绿即验无结构回归(结构不变、只加色)。
- **[结构快照变带色,diff 噪声]** 迁移时 diff 大(每帧加注解)→ 缓解:一次性人工重核(对眼并入),此后只 diff 配色变化。
- **[256/16 终端]** truecolor 在低色终端失真 → 缓解:1.0 默认 truecolor,降级表归路线图 1.4(`设计规范/01` 已记)。

## Migration Plan

新增 `theme.rs`;`render` 签名 + 上色 + `buffer_to_styled`(退 `buffer_to_text`);3 调用点改传 `&Theme`;6 `.snap` 迁移 + 新增 Daylight welcome。无数据迁移、不碰内核逻辑(纯 tui 视觉)。回滚 = revert(render 复原硬编码 + text 快照)。

## Open Questions

- `Theme::token_name` 对 `Modifier`(BOLD/DIM/REVERSED)是否一并注入快照 —— 倾向注入关键 Modifier(BOLD/REVERSED)以锁强调;实现期定粒度。
- 运行时主题切换的最终形态(config `theme` 字段 vs `/theme` 命令 vs 终端背景探测)—— 留后续 UX / 命令 change。
- cut2b-b 的新组件(diff/C7)直接复用 `Theme` + `buffer_to_styled`,其 themed 帧对眼在 cut2b-b 完成 6 帧。
