## MODIFIED Requirements

### Requirement: stdin y/n 权限 decider

系统 SHALL 提供 `StdinDecider`(impl `PermissionDecider`),用于 `--headless` CLI：对非 `ReadOnly`(`Network` / `Edit` / `Execute`)工具展示授权 prompt 后读取一行,经一个**可单测的纯解析函数**判定 —— `y` / `yes`(忽略大小写与首尾空白)→ `Allow`,其余(含空行 / EOF / read failure)→ `Deny`(fail-safe)。stdin 读取 MUST 为薄壳(异步下经 `spawn_blocking`),决策解析与读取解耦。

对 `Network` 工具,`StdinDecider` MUST 只消费 gate 传入的 tool-owned preview，不得按 tool name 重建 target / scope。`authorizable=false` 时 SHALL 完整输出 terminal-safe generic args 与 denial reason 后直接 Deny，不读取 stdin。有效 preview 的 prompt MUST 完整含工具名、lossless args、canonical initial target 与 preview scope，且不得截断；literal backslash 与被 escape 的 control / bidi / zero-width 字符 MUST 可区分。

有效 Network prompt MUST 先完整 format 为 bytes，再经可注入 writer 执行 `write_all` 与 `flush`。合法 short write SHALL 由 `write_all` 继续直至完整成功；serialization / format error、`write_all` 在部分进度后返回 `Err` / `WriteZero`、或 flush error 时 MUST 直接 Deny，reader 调用次数为 0。只有完整 prompt 成功 flush 后才可读取 stdin。`web_fetch` / `web_search` 的 redirect 次数直接取 preview，不硬编码；Deny / EOF 时由 gate 保证 `tool.execute` / `WebFetcher` 零调用。

对 `Edit` / `Execute`,既有 `tool requires confirmation: <name>`、`arguments: <args>`、`allow? [y/n]` 语义与 `parse_decision` 行为 MUST 保持不变。CLI 不提供 Network always-allow；CLI flags、stdout 模型流与 stdin decision grammar 不变。

#### Scenario: 确认输入 y 放行

- **WHEN** 解析用户输入 `"y"`(或 `"Y"` / `"yes"` / 带首尾空白)
- **THEN** 解析为 `PermissionDecision::Allow`

#### Scenario: 非确认输入拒绝(fail-safe)

- **WHEN** 解析用户输入为 `"n"` / 空行 / 其他任意串 / EOF
- **THEN** 解析为 `PermissionDecision::Deny`

#### Scenario: headless web_fetch 显示完整授权范围

- **WHEN** `StdinDecider` 处理一个 Network `web_fetch` 调用,URL 含长 path/query 与 terminal-unsafe 字符
- **THEN** 在读取 stdin 前输出完整、可逆 escaped args、初始 origin、最多 3 次可能跨站 redirect 与逐跳 SSRF 文案；不把 origin 写成持续授权

#### Scenario: headless web_search 使用同一 redirect scope

- **WHEN** `StdinDecider` 处理一个 Network `web_search` 调用,query 含 U+009B、U+202E、U+2066/U+2069、U+200B 或 literal `\u{...}`
- **THEN** 固定 DDG 初始目标与 web_search 自身最多 3 次可能跨站 redirect 均被说明；每个 unsafe scalar 显式 escape,literal backslash 仍可区分,完整 query 不截断

#### Scenario: headless Network 拒绝零 fetch

- **WHEN** Network prompt 后输入 `n`、EOF 或读取失败,并注入 counting `WebFetcher`
- **THEN** 返回 Deny,`tool.execute` 不运行,fetcher 调用数为 0

#### Scenario: 不可授权 preview 不读取 stdin

- **WHEN** Network preview 因未知工具、缺专用 preview、畸形参数或目标不可验证而 `authorizable=false`
- **THEN** stderr 输出 terminal-safe generic args 与原因后直接 Deny,reader 调用次数为 0；即使 Yolo / 异常 Allow 也不执行

#### Scenario: short write 完成与中途失败分流

- **WHEN** 分别注入每次只写少量 bytes 但最终成功的 short-chunk writer、写到 N bytes 后返回 Err / WriteZero 的 writer、以及 flush error writer
- **THEN** short-chunk writer 由 write_all 写完整并 flush 后才读取；其余三种均 Deny、reader 调用次数为 0、prompt 不被视为已呈现、工具不执行

#### Scenario: 成功 flush 严格先于 stdin read

- **WHEN** 注入记录调用顺序的成功 writer / reader,处理 authorizable Network preview
- **THEN** 顺序严格为 format → write_all → flush → read；只有最后的 y / yes 可 Allow

#### Scenario: Edit / Execute prompt 零回归

- **WHEN** `StdinDecider` 分别处理既有 Edit 与 Execute 调用
- **THEN** 工具名、arguments、`allow? [y/n]` 与 y/yes 解析语义保持不变,不出现 Network redirect 文案
