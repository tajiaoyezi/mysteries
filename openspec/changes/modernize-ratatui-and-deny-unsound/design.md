## Context

当前 manifest 使用 `ratatui = "0.29"` 与直接 `crossterm = 0.28`。根 lockfile 中 `ratatui 0.29.0` 是 `paste 1.0.15` 和 `lru 0.12.5` 的唯一反向依赖；直接 RustSec 审计把前者报告为 unmaintained、把后者报告为 `RUSTSEC-2026-0002` unsound。现有安全 workflow 会显示三项 allowed warning，但只对 vulnerability hard-fail。

`ratatui 0.30` 已拆成 `ratatui-core`、`ratatui-widgets` 和 backend crate。规划时当前 patch 为 `0.30.2`，`ratatui-core` 的 layout cache 使用不受该 advisory 影响的新 `lru`。其主 crate 虽同时暴露 `crossterm_0_28` / `crossterm_0_29` feature，但发布包对可选 `ratatui-crossterm` 依赖保留默认 features：只指定 `crossterm_0_28` 会实际同时解析 0.28/0.29，backend 按“最高已启用版本优先”选择 0.29。故要维持单一 crossterm，必须把项目直接依赖配套迁移到 0.29。0.30 的其他默认 features 还会额外启用 all-widgets、macros 等未使用能力，仍需关闭 defaults 后显式选择。另因 crossterm 0.29 的 default features 相比 0.28 新增 `derive-more`，迁移后的 lockfile 会增加 `derive_more` / `derive_more-impl`；这是上游 0.29 默认组合带来的已知传递依赖，不代表项目使用其 `is_*` helper，也必须在依赖 diff 中显式审查。

本 change 属于依赖、安全策略、TUI 外壳与 CLI 交互终端路径的跨域迁移。它不新增 headless 内核行为，AGENTS.md 的强制 RED→GREEN TDD 不适用；旧 lockfile 在 `--deny unsound` 下的失败是安全门禁基线，不是接口测试。视觉事实源仍为 `src/tui/theme.rs` 与既有 `insta` 快照，其次才是 `设计规范/`。

## Goals / Non-Goals

**Goals:**

- 迁移到 `ratatui 0.30.x`，消除 `paste 1.0.15` 与受影响 `lru 0.12.5` 路径。
- Ratatui 直接 feature 只启用现有产品需要的 crossterm 0.29 backend 与 layout cache，并把直接 crossterm 迁移到同一 0.29，避免双版本与其他 Ratatui 0.30 默认 feature 扩张；不为代码未使用的 underline color 重复 opt-in，并明确记录 crossterm 0.29 default 新增的 `derive-more` 传递依赖。
- 以最小 API / Buffer 适配维持 TUI 的渲染、布局、Unicode 宽度、鼠标选择、输入、权限框、markdown 和终端生命周期行为，并保持 CLI auth selector、隐藏输入、取消和 raw-mode 恢复。
- 让本地与 CI RustSec 审计对任一 unsound warning fail-closed，同时继续如实展示但允许 unmaintained warning。
- 用依赖图、直接审计、全量 Rust 门禁、受控快照迁移和 Windows Terminal 真机 TUI/CLI smoke 形成闭环证据。

**Non-Goals:**

- 不引入第二份 crossterm，不主动采用 bracketed `Event::Paste` 或改变事件批处理、ConPTY 粘贴、鼠标捕获与 terminal restore 语义。
- 不启用 `ratatui 0.30` 的 all-widgets、calendar、macros、其他 backend 或 unstable features。
- 不重设计 C1–C11，不调整 token、布局、文案、键位、状态机或设计规范；仅允许更新经审查确认由 Ratatui 0.30 造成的 `command_completion_snapshot` 迁移差异。
- 不处理 `syntect -> bincode 1.3.3` unmaintained warning，不启用 `--deny unmaintained` / `--deny warnings`。
- 不修改 GitHub Actions Node runtime、Action pin、workflow trigger / permission / install isolation 等既有门禁结构。
- 不修改 Agent Loop、Provider、Tool、Permission、Session、Config，亦不实现 cancellation、MCP 或 subagent。
- 不新增项目 MSRV 声明；仓库继续使用 `rust-toolchain.toml` 的 `stable`。

## Decisions

### 1. 使用 `ratatui 0.30` 显式最小 features，并配套统一到 `crossterm 0.29`

`Cargo.toml` SHALL 使用 `ratatui` 的 0.30 version requirement、关闭 default features，并只开启：

- `crossterm_0_29`：让 Ratatui backend 与项目配套升级后的直接 `crossterm 0.29` 使用同一版本；
- `layout-cache`：保留既有布局计算缓存的性能语义。

直接 `crossterm` 继续启用 crate defaults 与 `event-stream`，只把 version requirement 从 0.28 升到 0.29；这不等于 default feature 集逐字不变：0.29 新增默认 `derive-more`，会引入 `derive_more` / `derive_more-impl`。选择接受它是因为统一到 Ratatui backend 实际使用的 0.29 比保留双 crossterm 或拆分全部 Ratatui imports 风险更低；项目不新增 direct feature 来使用这些 helper。`ratatui-crossterm` 发布包自身 default 仍传递启用 underline-color，但项目不在 Ratatui 主 crate 上重复声明未直接使用的 `underline-color` feature。实施时以 lockfile 解析到的最新兼容 0.30 patch 为事实源并审查完整传递依赖 diff。`cargo tree -d` 与反向依赖查询必须证明只剩单一 `crossterm 0.29`，`paste` 消失，且 `lru` 不再匹配 `RUSTSEC-2026-0002`；lockfile 审查还必须把 `derive_more` 标为这次迁移的已接受上游传递变化。

替代方案：

- 直接 `ratatui = "0.30"` 使用全部新默认 features——弃；会无理由增加 calendar/macros 等能力面。
- 保持直接 `crossterm 0.28` 并启用 `crossterm_0_28`——弃；对发布包的真实 `cargo tree` 验证会同时解析 0.28/0.29，且 backend 仍选择 0.29，制造双版本和错误安全感。
- 绕过主 crate、直接依赖 `ratatui-core` / `ratatui-widgets` / `ratatui-crossterm(default-features=false)` 以保留 0.28——弃；会把全仓 `ratatui::...` import 拆成多 crate 重写，范围和维护成本高于配套升级。
- 用 `[patch]` 单独替换 `lru`——弃；绕过上游兼容矩阵、保留 `paste`，且把维护责任转移给本仓库。
- 关闭 layout cache——弃；虽可移除 `lru`，但会引入没有基准支持的 TUI 性能退化。

### 2. API 与 Buffer 迁移由编译器和回归测试共同暴露，禁止借机重构

先记录 manifest / lockfile / audit / snapshot 基线，再升级依赖。只修复由 0.30 模块化、trait/API 或 TestBackend Buffer 差异实际暴露的兼容点；每个非机械改动都必须能映射到既有 `tui-shell` requirement 或已批准的 0.30 迁移差异。不得把新 API 当作改布局、换 widget、调整宽度算法或清理大文件的授权。直接 crossterm 也被 `src/cli.rs` 的交互式认证路径使用，因此若编译器暴露兼容点，允许在那里做最小修复；即使不需要代码改动，也必须回归 selector、隐藏输入与 raw-mode 恢复。

视觉分类为“主体纯 port + 一个受控 adapt”：`设计规范/01-设计令牌.md`、`设计规范/02-布局与交互.md`、`设计规范/03-组件清单.md` 的 C1–C11 保持不变；唯一允许的 adapt 是 `mysteries__tui__render__tests__tui_command_completion.snap` 中 Ratatui 0.30 导致的相邻同 style run 合并，以及 `/models` 旧版缺字/留白变为完整命令描述。实施必须先以 `.snap.new` 展示精确 diff，确认没有区域、token、行数或其他文本变化，并取得用户批准后才可接受该单份快照；其他已跟踪 `.snap` 必须逐字节零 diff，最终不得遗留 `.snap.new`。

替代方案“为复刻 0.29 的缺字/留白编写兼容 hack”被否决，因为会把旧渲染缺陷固化进产品；替代方案“接受所有上游差异并批量 approve 快照”同样被否决。只接受上述单份、已知且经用户批准的迁移差异。

### 3. RustSec policy 只新增 `--deny unsound`

本地文档命令与 workflow 的绝对 binary 调用都增加 `--deny unsound`。任一 vulnerability 仍按 cargo-audit 默认行为失败；任一 unsound informational warning也失败。`unmaintained` 继续 report-only，因此 `syntect -> bincode 1.3.3` 可见但不会把本 change 扩成 syntax asset 重构。

不创建 `.cargo/audit.toml`，不增加 advisory ignore。workflow 继续使用固定 `cargo-audit 0.22.2`、隔离 `CARGO_HOME` / install root、绝对 binary 与显式 root lockfile；只改审计策略参数。`cargo-audit` 成功输出不会打印显式零计数，因此“0 vulnerability / 0 unsound”的证据定义为：实际命令含 `--deny unsound`、扫描指定根 lockfile并 exit 0，同时输出仍保留 allowed warning；人类报告必须把这个结论与“warning-free”区分，但不得要求原始 audit log虚构工具没有提供的零计数行。

替代方案 `--deny warnings` / `--deny unmaintained` 被否决，因为会强迫本 change 同时治理 `bincode`，破坏单一风险边界。只按 advisory ID 拒绝当前 `lru` 也被否决，因为无法阻止未来新的 unsound 依赖进入。

### 4. 以一组可复现的依赖、安全、行为证据验收

- **迁移前安全 RED**：当前 lockfile 由解析出的绝对 `cargo-audit` binary 执行 `audit --deny unsound --file Cargo.lock`，必须因 `RUSTSEC-2026-0002` 非零退出；普通 audit 仍展示 3 个 allowed warning，项目 Cargo alias 不参与证据。
- **迁移后依赖证据**：`cargo tree --locked -i paste` 无匹配；旧 `lru 0.12.5` 无匹配；`ratatui 0.29`、`crossterm 0.28` 与重复 crossterm 无匹配。
- **迁移后安全 GREEN**：同一绝对 `cargo-audit` binary 的 `audit --deny unsound` 命令 exit 0，由此证明 0 vulnerability / 0 unsound，并保留 `bincode` unmaintained warning；报告说明零值是由命令策略与退出状态推导，不伪称原始 log 含零计数。
- **行为证据**：定向 TUI / markdown / selection / viewport / CLI 交互测试、全量 `cargo test --locked`、clippy、release build 全绿；除经用户批准的单份命令补全快照外零 churn。
- **远端证据**：PR 上 Windows + Ubuntu CI 与 `security-audit` 都通过，audit log 可见 `--deny unsound` 且无 Node/runtime 或其他 workflow 重构夹带。

### 5. GitHub Actions Node24 警告留给紧随其后的独立 change

最新 CI log 确有 Node20 Action 被 runner 强制以 Node24 执行的警告，但当前 job 已成功，且其修复涉及 Action major、不可变 SHA、普通 CI 权限与 cache 行为。把它混入本次 Ratatui 迁移会同时改变 Rust dependency graph 和 CI 执行供应链，难以隔离失败归因。因此本 change 只改安全审计参数；Action runtime 另开 `modernize-github-actions-runtime`。

## Risks / Trade-offs

- **[0.30 backend feature 选错导致双 crossterm 或类型不兼容]** → 关闭 Ratatui defaults、显式 `crossterm_0_29`，直接 crossterm 同步 0.29；用 `cargo tree -d` 和全量 Windows/Linux build 证明单版本。
- **[`crossterm 0.29` 改变 Windows 事件/终端边界]** → 保留原 feature 集与所有事件路由；自动化只覆盖现有可注入的 Press/Release、event batch 与粘贴逻辑，mouse capture、EventStream live IO、raw mode / alt-screen restore 因当前无 test seam，明确由 Windows Terminal 真机复核，不用 0 tests 冒充。
- **[上游布局缓存或 Buffer 细节改变造成隐性视觉漂移]** → 保留 `layout-cache`，运行全部 `TestBackend + insta`、selection/width/viewport 定向测试；只允许经用户批准的单份命令补全快照差异，其他快照零 churn。
- **[编译修复演变为大规模 TUI 重构]** → 将编辑限定为编译器暴露的兼容点；审查 diff 时逐项要求迁移依据，拒绝无关 rename/cleanup。
- **[真终端行为未被 TestBackend 覆盖]** → Windows Terminal 真机覆盖启动、markdown、滚轮、选择复制、ConPTY 多行/大段粘贴、权限拒绝、模式切换和双 Ctrl+C 退出；另覆盖 `auth login` selector、隐藏输入、取消和 raw-mode 恢复。
- **[`--deny unsound` 因未来 database 新 advisory 让 CI 在 lockfile 未变时变红]** → 这是有意的 fail-closed；修复或经独立 OpenSpec change 治理，不降级为 warning 或 ignore。
- **[0.30 patch 的 Rust requirement 高于旧 0.29]** → 仓库明确跟随 `stable` 且当前 toolchain 高于上游 requirement；CI 双平台 build 是事实验证。本 change不宣称固定 MSRV。
- **[传递依赖 churn 较大]** → 只允许 Ratatui 模块化与 feature 选择直接造成的 lockfile diff；任何无关 `cargo update` 变更必须回退。
- **[RustSec database 在 apply 前漂移]** → 规划时的 0 vulnerability / 3 warning 只是 2026-07-11 基线；若 live database 新增或重分类 advisory，立即停止并回到 proposal/spec 评估范围，不通过 ignore 或硬套旧计数继续。

## Migration Plan

1. 保存当前 `cargo tree`、普通 audit、`--deny unsound` RED、TUI 定向测试与 snapshot clean 基线。
2. 修改 `Cargo.toml` 的 Ratatui version/features和直接 crossterm version，并以定向 lockfile update 解析 Ratatui 0.30.x / crossterm 0.29；立即审查 manifest/lock diff和反向依赖树。
3. 运行 `cargo check --all-targets --locked`，只处理编译器暴露的 Ratatui/Crossterm 兼容点；随后跑定向 TUI/事件/CLI 测试，逐项审查已知命令补全快照差异并在用户批准后接受该单份快照，其他快照保持零 churn。
4. 将本地文档和 security workflow 接入 `--deny unsound`；本地正向与旧 lockfile 负向验证都直接调用已解析的绝对 `cargo-audit` binary，避免 Cargo alias shadow。
5. 运行 fmt、clippy、全量 test、release build、strict OpenSpec validation；再做真机 smoke、PR checks 与 post-merge手动审计。

回滚应整体 revert manifest、lockfile、API 适配、workflow参数与文档。回滚会重新引入已知 unsound warning，只可用于定位回归，不可作为可发布最终状态；若 0.30 迁移无法保持行为，应修正 feature/API 适配或提出新的依赖替代 change，不以 advisory ignore 收场。

## Open Questions

- 无阻塞问题。`modernize-github-actions-runtime`、`bincode` unmaintained 治理与通用 Agent cancellation 继续保持独立后续 change。
