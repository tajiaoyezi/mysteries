## ADDED Requirements

### Requirement: crossterm 0.29 迁移保持 auth 交互终端契约

直接 crossterm 从 0.28 迁移到 0.29 后，CLI SHALL 保持既有 `AuthPrompter` 终端实现、纯按键归约与凭据写入边界。`auth login` / `auth logout` 的 provider、登录方式、协议与凭据 selector SHALL 继续只处理 Press 事件，支持方向键首尾环绕、Enter 确认以及 Esc / Ctrl+C 取消；隐藏 key 输入 SHALL 支持字符、Backspace / Delete、Enter 与取消，MUST NOT 回显明文。任一取消、EOF 或读取错误 MUST NOT 持久化半份配置或凭据。交互结束后 raw mode MUST 完整恢复。若 crossterm 0.29 暴露编译/API 兼容点，实施只允许在 `src/cli.rs` 做最小调整，CLI flags、配置格式、credential 格式与 auth 流程语义 MUST NOT 改变。

#### Scenario: 可注入按键归约保持全绿

- **WHEN** 运行 `apply_secret_key`、`apply_select_key` 及注入 `AuthPrompter` 的现有 auth 单测，覆盖 Press / Release、方向键环绕、Enter、Backspace、Esc 与 Ctrl+C
- **THEN** 非 Press 事件被忽略，选择、隐藏输入、确认与取消结果与迁移前一致，测试不依赖真实终端、网络或用户凭据

#### Scenario: Windows Terminal auth 真机恢复

- **WHEN** 在 Windows Terminal 真机进入 `auth login` 的交互式 provider/model selector 与隐藏 key 输入，并分别用 Esc / Ctrl+C 取消
- **THEN** 方向键、Enter、Backspace、隐藏字符回显与 Press-only 过滤保持正常，取消不持久化测试凭据，退出后 raw mode 完整恢复且 PowerShell 输入可立即继续

#### Scenario: CLI 变更范围保持最小

- **WHEN** 审查 crossterm 0.29 迁移后的 `src/cli.rs` 与 CLI 测试 diff
- **THEN** 只允许编译器、API 或回归测试实际暴露的兼容修改，不改变 CLI flags、auth 候选顺序、配置 / credential 持久化格式或错误文案
