# Changelog

本项目遵循 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.1.0/) 与 [语义化版本](https://semver.org/lang/zh-CN/)。
条目由 git 历史与 `.ai_history/logs/` 决策记录蒸馏而来。

## [Unreleased]

## [1.3.1] - 2026-07-23

> v1.3 功能集的首次公开交付；收口 Agent execution scope 与单层只读 subagent，不引入 config/session wire 迁移或新的写入、Network child 能力。

### 新增
- **Agent execution scope 基础**:每次 Agent run 具有稳定 identity、可传播 cancellation、iteration/deadline/child-depth 预算和单调收窄的工具/权限 capability;既有入口保持兼容,TUI Interrupt 改由 Agent Loop 内核收口 history,Provider 回复前中断会隔离未提交的旧 Prompt,避免下一轮继续旧任务。受限 `ToolRegistry` 共享同一工具实例,scope clamp 先于权限模式、allowlist 与用户决策生效;取消不硬终止已进入 blocking pool 的 OS 同步工作。
- **单层只读 `delegate_task`**:TUI与headless可把workspace调研委派给临时child Agent;child只含`list_dir` / `read_file` / `glob` / `grep`,以canonical path限制在当前workspace（包含walker加载的parent、`.ignore`与`.gitignore`规则文件）,不复制parent history,结果用untrusted envelope返回。outer同时active的child固定≤4,但单轮delegate occurrence总数不设硬上限;每个child最多8次tool-enabled Provider调用加1次forced-final,从invocation开始以120秒覆盖preflight和完整run,成本放大上界为`delegate occurrence数 × 最多9次Provider调用`。本MVP不含递归、后台任务、child session、跨child token总预算、写入或Network child;child-only扫描字节也无硬上限。四fs工具继续先读取 / 遍历 / 收集；仅`read_file`与`grep`按输出cap后置截断，`list_dir`与`glob`的既有输出没有该硬上限。

### 修复
- **Release publish identity**:draft create改为从官方REST `201`响应捕获numeric Release ID与`upload_url`;三个asset upload、draft identity验证及public PATCH全程绑定该ID，移除post-create list rediscovery failure window。

## [1.3.0] - 2026-07-23

> annotated tag、attempt 1失败run与非公开draft/assets均按原状保留；该版本未成为public/latest Release且已消耗、不得复用，公开latest仍为v1.2.0。

## [1.2.0] - 2026-07-13

> 首个通过固定 revision、双平台原生构建、checksums 与公开下载复核交付的自动化 GitHub Release。

### 新增
- **有界并行安全工具**:同一模型回复中连续的 `list_dir` / `read_file` / `glob` / `grep` 以 per-Agent 上限 4 并行执行;独立 `ToolConcurrency` 元数据(默认 `Exclusive`),不从 `PermissionLevel` 推断。结果仍按模型 occurrence 顺序写入 history / TUI;Network / Edit / Execute / Plan / 交互工具保持串行屏障。
- **并行批次 TUI**:多张 Running C5 工具卡 + 活动状态行 `并行执行 N 个工具…` / `处理 N 个工具（最多并行 4）…`;Interrupt 收口 Running 卡与未配对 tool result;旧 session 激活前规范化残留 Running / dangling call。
- **Network 权限级**:`web_fetch` / `web_search` 从 `ReadOnly` 拆为 `Network`;TUI 与 `--headless` 展示 terminal-safe 完整参数、canonical target 和 redirect/SSRF scope。有效 preview 默认逐次询问,Yolo 仅自动放行可授权 preview;未知/畸形调用 fail-closed,SSRF 仍逐跳强制。Provider transport 不属于 Tool gate。
- **Plan 进度持久化**:`SessionLine::Plan` 把 `current_plan` 随会话 jsonl 落盘;`--resume` / `--continue` 经 plan-only seam `apply_loaded_plan` 做**纯视觉恢复**(不执行续接)。**降级不兼容**:升级后写出的含 `Plan` 行会话,回退到旧 v1.1.0 二进制读取会 `Err`。
- **CLI `--help` / `-h` / `--version`**:输出用法 / 版本号后退出;此前这些及任意未知 flag 都会静默进入 TUI。
- **RustSec 依赖安全门禁**:新增独立、最小权限的 `security-audit` workflow，在 PR、`master` push、每周 schedule 与手动触发时用隔离安装的固定 `cargo-audit` 审计已提交的根 `Cargo.lock`；vulnerability、kind=`unsound` warning 与审计基础设施错误阻断，其他 informational warning 保持可见。

### 变更
- README tests badge 去数字化(`800+`),不再硬编码具体数,避免与实际测试数漂移。
- 四个本地读取工具的同步文件工作经 `spawn_blocking` + 进程级 Semaphore(4) offload,避免占满 Tokio worker。
- `syntect` 从 `default-fancy` 收窄为实际使用的默认 syntax/theme + `regex-fancy`，移除未使用的 plist/yaml/html loader 依赖面。
- TUI 迁移到最小 feature 的 `ratatui 0.30` + 单一 `crossterm 0.29`，移除 unmaintained `paste 1.0.15` 与受 `RUSTSEC-2026-0002` 影响的 `lru 0.12.5` 路径；RustSec 门禁增加 `--deny unsound`，vulnerability / unsound 均阻断。`syntect -> bincode 1.3.3` unmaintained warning 继续可见但不阻断，不宣称 warning-free。

### 安全
- 修复 `crossbeam-epoch 0.9.18` 的 `RUSTSEC-2026-0204`，并通过移除未使用的 `plist -> quick-xml 0.39.4` 路径修复 `RUSTSEC-2026-0194` / `RUSTSEC-2026-0195`；Ratatui 迁移同时消除 `RUSTSEC-2026-0002`，当前根 lockfile 为 0 vulnerability / 0 unsound。

### 测试
- 新增 CLI flag 解析 5 组单测(`wants_help` / `wants_version` / `help_text` / `version_text`);`tui::width` CJK 显示宽度补 8 组 characterization 测试(全 / 半角边界、零宽标记、截断),经 mutation check 防假绿。

## [1.1.0] - 2026-07-06

> 开发里程碑；当时未创建 Git tag 或 GitHub Release。历史源码参考 commit `271d4ee67954dce7ea242144a0268adfd0cd4d61`。

### 新增
- **思考模式**:统一 `Depth`(off/low/medium/high/xhigh)抽象,向下映射到 Anthropic adaptive+effort 与 OpenAI `reasoning_effort` 双链;`/think` 命令切换;思考过程 TUI 折叠展示(默认展开、超阈值折叠溢出、`✻ 思考` header);footer 档位指示。
- **Plan 模式(L1)**:`PermissionMode::Plan` + schema-omit(plan 期只下发只读工具)+ `submit_plan`(结构化计划 + 每步验收)+ `ask_user`(A/B/C 澄清)+ 批准即执行;常驻进度面板 + `update_plan` 逐步上报。
- **联网工具**:`web_fetch` / `web_search`,含 SSRF 内网/重定向护栏。
- **会话持久化**:jsonl 快照 + `--resume` 续最近 + 会话选择器 + Ctrl+C 双击退出守卫。
- **命令白名单** + always-allow 权限工效。
- **TUI 渲染**:Assistant markdown 渲染(CommonMark+GFM,syntect 语法高亮,简易表格 CJK 对齐)、工具卡 diff 高亮、大粘贴折叠占位符、消息排队(两级取消)、多行输入、鼠标拖选复制、滚轮滚动、输入历史、jump-to-bottom。

### 变更
- 上下文压缩默认启用(内置模型窗口表)。
- `reqwest` 启用 `socks` 特性:运行时 `all_proxy=socks5://...` 代理可用。
- 全仓 rustfmt 基线 + 钉 stable toolchain;新增 CI(fmt/clippy/全量 test/build)。

### 修复
- 大粘贴跨批合并、防逐行误提交、首字符泄漏、粘贴延迟等一系列 ConPTY 交互问题。
- `run_shell` 子进程冲掉鼠标捕获致滚轮失效。
- 集成测试工具数断言随默认注册表更新。

## [1.0.0] - 2026-06-27

> 开发里程碑；当时未创建 Git tag 或 GitHub Release。历史源码参考 commit `8c99d0dda69eb5648d7e7ec8871179f73794d439`。

### 新增
- 自研 **Agent Loop**:模型决策 → tool_calls 逐个过权限门 → 执行回传 → 续推;`max_iterations` 上限、Esc 中断、全事件入 history。
- **7 个内置工具**:`list_dir` / `read_file` / `glob` / `grep` / `write_file` / `edit_file`(唯一匹配替换)/ `run_shell`。
- **三级权限 × 三种模式**:工具按 `ReadOnly/Edit/Execute` 分级;`Normal/AcceptEdits/Yolo` Shift+Tab 循环;写/执行类内联确认。
- **多 Provider**:OpenAI 兼容(含 WPS AI)/ Anthropic / Mock,统一 `Provider` trait + registry + `/models` 热切换,流式解析自实现。
- **上下文管理**:`ContextStrategy`(Passthrough / Compacting)+ token 用量显示。
- **TUI 外壳**:ratatui + crossterm,双 task + channel 架构,Midnight/Daylight 双主题。
- **凭据链**(env → 文件)、**配置分层**(用户级 + 项目级合并)、**首次运行引导**。

[Unreleased]: https://github.com/tajiaoyezi/mysteries/compare/v1.3.0...HEAD
[1.3.0]: https://github.com/tajiaoyezi/mysteries/tree/v1.3.0
[1.2.0]: https://github.com/tajiaoyezi/mysteries/releases/tag/v1.2.0
[1.1.0]: https://github.com/tajiaoyezi/mysteries/commit/271d4ee67954dce7ea242144a0268adfd0cd4d61
[1.0.0]: https://github.com/tajiaoyezi/mysteries/commit/8c99d0dda69eb5648d7e7ec8871179f73794d439
