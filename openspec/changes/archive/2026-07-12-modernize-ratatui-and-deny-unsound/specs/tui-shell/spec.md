## ADDED Requirements

### Requirement: ratatui 0.30 最小依赖迁移保持 TUI 契约

TUI SHALL 使用 `ratatui 0.30.x`，并在 manifest 中关闭其 default features，只显式启用 `crossterm_0_29` 与 `layout-cache`。项目直接 `crossterm` MUST 配套使用 0.29 并继续启用 crate defaults + `event-stream`，依赖图 MUST NOT 同时解析 `crossterm 0.28`，MUST NOT 启用未使用的 all-widgets / calendar、macros、其他 backend 或 unstable Ratatui features；代码未直接使用 underline color，故 Ratatui 主 crate MUST NOT 重复 opt-in `underline-color`（`ratatui-crossterm` 上游 default 的传递启用不属于该直接 feature 集）。crossterm 0.29 default 新增的 `derive-more` 及 `derive_more` / `derive_more-impl` 传递依赖属于本次已接受的上游 feature 变化，MUST 在 lockfile diff 中显式记录，但项目 MUST NOT 为使用其 helper 新增 direct feature。迁移后的 lockfile MUST 移除 `ratatui 0.29`、`paste 1.0.15` 与受 `RUSTSEC-2026-0002` 影响的 `lru 0.12.5`；任何仍由 Ratatui layout cache 使用的 `lru` 版本 MUST 不匹配该 unsound advisory。

迁移只允许为 `ratatui 0.30` / `crossterm 0.29` 编译、API 与回归测试暴露的 Buffer 兼容所需的最小代码调整。既有 `tui-shell` 全部 requirement（含 Press-only 键过滤、EventStream 批处理、Windows ConPTY 粘贴启发式、鼠标捕获、raw mode / alt-screen restore）、`src/tui/theme.rs` 语义 token，以及 `设计规范/01-设计令牌.md`、`设计规范/02-布局与交互.md`、`设计规范/03-组件清单.md` 的 C1–C11 布局、样式、状态和交互 SHALL 保持不变。本 requirement 的视觉分类为主体纯 port；唯一允许的 adapt 是 `mysteries__tui__render__tests__tui_command_completion.snap` 中相邻同 style run 合并，以及 `/models` 旧版缺字/留白改为完整命令描述。该单份快照 MUST 先以 `.snap.new` 展示精确 diff并取得用户批准后才能接受；其他全部已跟踪 `insta` 快照 MUST 零 diff，最终 MUST NOT 遗留 `.snap.new`。

#### Scenario: manifest 只启用所需的 Ratatui 能力

- **WHEN** 检查 `Cargo.toml` 中的 `ratatui` dependency declaration
- **THEN** Ratatui version requirement 为 `0.30`、`default-features = false`，直接 features 集合恰含 `crossterm_0_29`、`layout-cache`；直接 crossterm version 为 `0.29` 且继续启用 defaults + `event-stream`；不含 `underline-color`、all-widgets / calendar、macros、其他 backend 或 unstable Ratatui feature，lockfile 明确记录 crossterm 0.29 default 新增的 `derive_more` 传递路径

#### Scenario: lockfile 移除旧 soundness 和维护风险路径

- **WHEN** 对迁移后已提交的根 `Cargo.lock` 运行 `cargo tree --locked`、`cargo tree --locked -d` 与针对 `ratatui` / `crossterm` / `paste` / `lru` 的反向依赖查询
- **THEN** 只解析 `ratatui 0.30.x` 与单一 `crossterm 0.29.x`，不存在 `ratatui 0.29`、`crossterm 0.28`、`paste 1.0.15` 或 `lru 0.12.5`；layout cache 若仍引入 `lru`，其版本不匹配 `RUSTSEC-2026-0002`

#### Scenario: 四区与全部组件完成受控快照迁移

- **WHEN** 在 Midnight / Daylight 下用既有 `ratatui::backend::TestBackend` 运行覆盖欢迎态、流式 Assistant markdown、工具卡、C6 权限框、C10 状态、C11 输入、picker、Plan 与会话恢复的全部 `insta` 测试
- **THEN** 只有 `mysteries__tui__render__tests__tui_command_completion.snap` 可包含已审查且经用户批准的相邻同 style run 合并与 `/models` 完整命令描述差异；其他已跟踪 `.snap` 内容逐字节不变，最终没有 `.snap.new`，C1–C11 的区域、边框、glyph、语义颜色和行数均与迁移前一致

#### Scenario: Buffer 与可注入事件逻辑回归保持全绿

- **WHEN** 运行既有 selection text、Unicode width、viewport clipping、scroll、multiline input、paste、permission modal、Press/Release 过滤与 event batch 相关单测和集成测试
- **THEN** 测试全部通过，Press/Release 过滤、宽字符延续格、光标位置、视口跟底、滚轮/方向键、选择复制、权限应答与粘贴折叠均不因 Ratatui/Crossterm 迁移改变；不得把无 test seam 的真实 terminal lifecycle 宣称为本场景已覆盖

#### Scenario: Windows Terminal 生命周期真机等价

- **WHEN** 使用迁移后的 release binary 在 Windows Terminal 真机进入 TUI，执行键盘输入、ConPTY 多行/大段粘贴、滚轮、鼠标选择、Esc 中断与双 Ctrl+C 退出
- **THEN** EventStream 持续收事件，粘贴不误提交、折叠和尾流收口保持正常且后续键盘仍可输入，mouse capture、raw mode 与 alt-screen 在运行时正常并在退出后完整恢复，终端不残留鼠标捕获、raw mode、alternate screen 或不可见光标；该场景 MUST 以真机记录验收，不得用零测试 filter 或 TestBackend 冒充

#### Scenario: 无关模块和持久化格式不受影响

- **WHEN** 完成迁移并审查代码与测试 diff
- **THEN** Agent Loop、Provider、Tool、Permission、Session、Config、CLI flags、session JSONL 与用户配置格式均无行为变更；TUI 编辑只对应编译器、API 或回归测试实际暴露的 0.30/0.29 兼容点与经批准的单份命令补全快照迁移
