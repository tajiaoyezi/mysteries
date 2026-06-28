## ADDED Requirements

### Requirement: transcript 视口渲染保真(可见行数对齐视口高度)

`render` 渲染 transcript 时,实际占用的**屏幕行数 MUST 等于 `visible_transcript_lines` 切出的逻辑行数**且 MUST ≤ 视口高度。`visible_transcript_lines` 已按**预换行后**的行数精确切出 `viewport_lines` 行(`skip(offset).take(viewport_lines)`),故渲染 MUST NOT 依赖 `Paragraph` 的二次换行:`render_transcript` MUST NOT 对 transcript `Paragraph` 施加 `.wrap`。所有需要换行才完整可读的 transcript 行(`User` / `Assistant` 文本、展开态工具卡 `output` 体)MUST 在进入切片**前**按显示宽度 ≤ 视口宽度**预换行**;装饰边框(工具卡脚 / `Error`/`Help`/`Status` 的 `┌─…`/`└──…`)MUST 按渲染 `width` **自适配**生成,使每边框行显示宽度 ≤ 视口宽度、占恰好 1 个屏幕行(不二次换行、不增加屏幕行数);整行工具卡头等无法预换行的固定宽装饰在更窄终端 MAY 被**右端截断**。当 `follows_bottom` 为真时,transcript 的**最新(底部)内容 MUST 在视口内可见**,MUST NOT 因二次换行溢出被裁到视口下方。本不变量 MUST 可经 `ratatui::backend::TestBackend`(窄宽 + 超视口多块内容,含会触发二次换行的行 / 长工具输出)断言。

#### Scenario: 超视口内容跟随底部时最新内容可见

- **WHEN** 在窄 `TestBackend`(宽 < 80)上渲染一个预换行后总行数超视口、且含会触发 `Paragraph` 二次换行的行(如 80 宽边框块 / 超宽长工具输出)的 transcript,`follows_bottom` 为真,末块为含可识别串的最新 `User` / `Assistant` 内容
- **THEN** 渲染输出**包含**该末块(最新内容)的可识别串(底部不被裁),且 transcript 区实际屏幕行数不超过视口高度

#### Scenario: 边框按 width 自适配占 1 屏幕行不顶高

- **WHEN** 在宽 < 80 的 `TestBackend` 上渲染一个含装饰边框的块(如 `Error` 致命错误框)
- **THEN** 边框行按渲染 `width` 自适配生成,占据**恰好 1 个屏幕行**,MUST NOT 被二次换行成 2 行而把后续(更新)内容向下挤出视口

#### Scenario: 展开态工具输出长行预换行

- **WHEN** `tools_expanded` 为真,一个 `Tool` 卡的 `output` 含显示宽度超视口宽度的长行,渲染到窄 `TestBackend`
- **THEN** 该长行在进入切片**前**已按 ≤ 视口宽度预换行为多个逻辑行(内容不被整行截断丢失),且 transcript 区实际屏幕行数仍等于切出的逻辑行数(不依赖 `Paragraph` 二次换行)
