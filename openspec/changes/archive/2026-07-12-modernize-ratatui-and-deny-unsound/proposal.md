## Why

当前 `ratatui 0.29.0` 经 `lru 0.12.5` 引入 `RUSTSEC-2026-0002` unsound warning，并经已停止维护的 `paste 1.0.15` 扩大依赖面；现有 RustSec 门禁只展示这些 warning，仍允许相同类别重新进入主线。上一轮依赖安全加固已把 `ratatui` 迁移与 `--deny unsound` 明确留作独立后续 change，现在应在进入更大的 Agent cancellation / subagent 工作前关闭这条 soundness 债务。

## What Changes

- 将直接依赖从 `ratatui 0.29` 迁移到当前 `0.30.x`，并把直接 `crossterm 0.28` 配套迁移到 Ratatui backend 实际选择的单一 `0.29`；Ratatui 直接 feature 只启用 `crossterm_0_29` 与 layout cache，不启用未使用的 calendar / macros 等默认能力（backend 上游默认保留的 underline-color 不重复 opt-in）。
- 只做 `ratatui 0.30` / `crossterm 0.29` 编译、API 与 Buffer 兼容所需的最小改造；TUI 四区布局、主题 token、C1–C11 组件、输入事件/权限/滚动/选择/粘贴/markdown/终端恢复行为保持不变。已知 `command_completion_snapshot` 会因 0.30 的相邻同 style run 合并与旧版命令描述截断修正产生受控差异，该差异必须逐项审查并经用户批准，禁止扩张为其他 UI 改造。
- 使 lockfile 不再包含 `paste 1.0.15` 和受 advisory 影响的 `lru 0.12.5`；若新 `ratatui` 仍使用 `lru`，其解析版本必须不匹配 `RUSTSEC-2026-0002`。
- 将本地与独立 `security-audit` workflow 的 RustSec 命令提升为绝对 `cargo-audit` binary 的 `audit --deny unsound`：任一 unsound informational advisory 必须阻断，unmaintained warning 继续可见但首版不阻断，项目 Cargo alias 不参与安全证据。
- 以既有 `TestBackend + insta`、完整 Rust 质量门禁、依赖图、直接 RustSec 审计和真机 TUI/CLI smoke test 证明迁移没有未解释的行为或视觉回归；除上述已知、经用户逐项批准的命令补全快照外，不得批量 approve 或改写快照来适配依赖升级。
- 同步 `CONTRIBUTING.md`、README 与 CHANGELOG 的依赖安全命令、门禁语义和剩余 warning 说明。
- 明确不在本 change：引入第二份 crossterm、主动采用新的 `Event::Paste` 路径或改变 ConPTY 输入模型、治理 `syntect -> bincode 1.3.3`、阻断全部 unmaintained warning、升级 GitHub Actions Node runtime、改变项目发布版本、重设计 UI、修改 Agent Loop / Provider / Tool / Permission / Session / Config，或实现 cancellation / subagent。

## Capabilities

### New Capabilities

- 无。

### Modified Capabilities

- `dependency-security`：把 unsound informational advisory 从“仅展示”提升为本地与 CI hard-fail，同时保留 unmaintained warning 的可见、非阻断语义。
- `tui-shell`：将 TUI 外壳迁移到显式最小 feature 的 `ratatui 0.30.x` + 单一 `crossterm 0.29.x`，锁定现有渲染和终端交互，并把快照变化限制为已知、经审查批准的 `command_completion_snapshot` 迁移差异。
- `cli-runtime`：直接 crossterm 迁移到 0.29 时，保持 `auth login` / `auth logout` 的 selector、隐藏输入、取消与 raw-mode 恢复契约，并允许 `src/cli.rs` 中实际需要的最小兼容修复。

## Impact

- **依赖**：`Cargo.toml`、`Cargo.lock`；直接依赖仍为 `ratatui` + `crossterm`，前者接受 0.30 模块化拆分产生的 `ratatui-core` / `ratatui-widgets` / `ratatui-crossterm` 传递依赖，后者配套升级到单一 0.29；同时明确接受 crossterm 0.29 default features 新增的 `derive-more` 及其 proc-macro 传递依赖，不新增其他直接 crate。
- **代码 / 测试**：`src/tui/` 内受 `ratatui 0.30` 编译/Buffer 行为影响的最小位置、受直接 crossterm 0.29 影响的 `src/cli.rs` 交互输入路径，以及对应单测 / `insta` 快照；不得借迁移重构无关状态机。
- **安全门禁**：`.github/workflows/security-audit.yml` 的绝对 `cargo-audit` 调用增加 `--deny unsound`，既有最小权限、隔离安装、触发条件与 fail-closed 语义不变。
- **文档**：`CONTRIBUTING.md`、README、CHANGELOG `[Unreleased]`。
- **视觉契约**：完整 port `设计规范/01-设计令牌.md` 的语义颜色、`设计规范/02-布局与交互.md` 的四区/状态机，以及 `设计规范/03-组件清单.md` 的 C1–C11；仅 `command_completion_snapshot` 为经审查批准的受控 adapt，其余无 adapt、无 drop，也不以 `UI设计/*.html` 作为 diff 基准。
- **兼容性**：CLI flags、配置、session wire、权限模型、公开运行时行为均不变；crossterm 事件批处理、Windows ConPTY 粘贴启发式、TUI 鼠标/terminal restore 以及 `auth login` 的 selector、隐藏输入和 raw-mode 恢复必须由自动化 + 真机证明等价。依赖自身要求的 Rust 版本由仓库既有 `stable` toolchain 满足，本 change 不新增项目 MSRV 声明。
