# Changelog

本项目遵循 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.1.0/) 与 [语义化版本](https://semver.org/lang/zh-CN/)。
条目由 git 历史与 `.ai_history/logs/` 决策记录蒸馏而来。

## [Unreleased]

### 新增
- **Network 权限级**:`web_fetch` / `web_search` 从 `ReadOnly` 拆为 `Network`;TUI 与 `--headless` 展示 terminal-safe 完整参数、canonical target 和 redirect/SSRF scope。有效 preview 默认逐次询问,Yolo 仅自动放行可授权 preview;未知/畸形调用 fail-closed,SSRF 仍逐跳强制。Provider transport 不属于 Tool gate。
- **Plan 进度持久化**:`SessionLine::Plan` 把 `current_plan` 随会话 jsonl 落盘;`--resume` / `--continue` 经 plan-only seam `apply_loaded_plan` 做**纯视觉恢复**(不执行续接)。**降级不兼容**:升级后写出的含 `Plan` 行会话,回退到旧 v1.1.0 二进制读取会 `Err`。
- **CLI `--help` / `-h` / `--version`**:输出用法 / 版本号后退出;此前这些及任意未知 flag 都会静默进入 TUI。

### 变更
- README tests badge 去数字化(`800+`),不再硬编码具体数,避免与实际测试数漂移。

### 测试
- 新增 CLI flag 解析 5 组单测(`wants_help` / `wants_version` / `help_text` / `version_text`);`tui::width` CJK 显示宽度补 8 组 characterization 测试(全 / 半角边界、零宽标记、截断),经 mutation check 防假绿。

## [1.1.0] - 2026-07-06

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

### 新增
- 自研 **Agent Loop**:模型决策 → tool_calls 逐个过权限门 → 执行回传 → 续推;`max_iterations` 上限、Esc 中断、全事件入 history。
- **7 个内置工具**:`list_dir` / `read_file` / `glob` / `grep` / `write_file` / `edit_file`(唯一匹配替换)/ `run_shell`。
- **三级权限 × 三种模式**:工具按 `ReadOnly/Edit/Execute` 分级;`Normal/AcceptEdits/Yolo` Shift+Tab 循环;写/执行类内联确认。
- **多 Provider**:OpenAI 兼容(含 WPS AI)/ Anthropic / Mock,统一 `Provider` trait + registry + `/models` 热切换,流式解析自实现。
- **上下文管理**:`ContextStrategy`(Passthrough / Compacting)+ token 用量显示。
- **TUI 外壳**:ratatui + crossterm,双 task + channel 架构,Midnight/Daylight 双主题。
- **凭据链**(env → 文件)、**配置分层**(用户级 + 项目级合并)、**首次运行引导**。

[1.1.0]: https://github.com/tajiaoyezi/mysteries/releases/tag/v1.1.0
[1.0.0]: https://github.com/tajiaoyezi/mysteries/releases/tag/v1.0.0
