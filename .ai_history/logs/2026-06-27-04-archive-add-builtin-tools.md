# 2026-06-27 · 04 · archive add-builtin-tools

## 决策

- **实装 §12 第 2 步剩余半的 7 个实体工具**(fs/edit/shell),接 change A 的 Tool / ToolRegistry / 权限门 | 主导:范围内自然项(2-split 的「工具」批)| 依据:change design.md DB1
- **registry 防重名:保 `Vec` + `register → Result`,不换 `HashMap`** | 弃:HashMap(不保序,`schemas()` 顺序确定性需引 `indexmap` 新依赖,不值)| 主导:子 agent,审查确认 | 依据:DB2(解决 change A 审查遗留的 ② 项)
- **run_shell:`tokio::process` + `timeout` + `kill_on_drop` 防孤儿 + 跨平台 cmd/sh + 固定 content 格式** | 依据:DB7
- **截断 UTF-8 安全(取 char 边界,不裸切)** | 依据:DB5
- **ReadOnly 工具路径不限定 cwd(可读任意用户可读文件、无权限门)= 已知风险,1.0 接受,收口点 §13 1.3 PolicyEngine** | 主导:主 agent 审查发现 → 要求文档化(不加限定代码)| 依据:DB13
- **grep 遇非 UTF-8 / 不可读文件跳过(不整体失败)** | 主导:主 agent 审查发现 bug → 出修复 prompt | 依据:本轮 fix + 回归测试 `grep_skips_non_utf8_files_and_returns_text_matches`
- 其余(工具自吞错误为 is_error、edit_file 唯一匹配且失败保文件、main 不接 Loop 留 transport)见 design.md DB1–DB13

## 变更

- `src/tool/{fs,edit,shell}.rs` 实装 7 工具;`tool/mod.rs` register 防重名(返回 `Result`)
- 新增依赖:`ignore` / `globset` / `regex`;`tokio` += `process` / `time`;dev-dep `tempfile`
- 审查修正:grep/glob/list_dir 的 walk Err 与 grep 的 read Err 改为跳过;新增 grep 非 UTF-8 回归测试;design.md 加 DB13
- 验证:`cargo test` 48 passed;dead_code warning(`main` 未接 Loop,Non-Goal,接受)
- archive:`changes/add-builtin-tools` → `changes/archive/2026-06-27-add-builtin-tools`;`specs/` 新增 `builtin-tools`,`tool-system` ADDED(register 防重名)

## 待决

- transport change:reqwest + SSE + 超时/重试 + 凭据链 + Anthropic;之后 `main` 接 Loop + stdin y/n decider + 拆 `src/lib.rs` 起 `tests/` 端到端
- **1.3 PolicyEngine**:ReadOnly 工具读路径限定(DB13 收口点)
- `run_shell` 命令解析仅走 shell `-c`/`/C`(argv 形式留后续)

## 引用

- change:`add-builtin-tools`(rationale / rejected alternatives 全量见 design.md DB1–DB13;archive 路径 `changes/archive/2026-06-27-add-builtin-tools`)
- 技术方案 §5.3 / §5.4 / §10 / §11 / §13
- 前置 change:`add-agent-loop-core`(决策记录 2026-06-27-03)
- session log:无专属 checkpoint —— 子 agent propose / implement;主 agent 负责 review(抓出 ①② 并出修复 prompt)与 commit/archive
