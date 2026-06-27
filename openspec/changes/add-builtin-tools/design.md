## Context

本 change 实装技术方案 §12 第 2 步剩余半的 7 个实体工具,接到 change A 已落地的 `Tool` / `ToolRegistry` / 权限门 / Agent loop 之上。已读真实代码并按权威次序(code > spec)对齐:

- `Tool::execute(&self, args: Value, ctx: &ToolContext) -> ToolOutcome`(**返回 `ToolOutcome` 非 `Result`**)→ 工具须自吞错误为 `is_error` outcome,不向外抛。
- `ToolRegistry { tools: Vec<Box<dyn Tool>> }`,`register` 现返回 `()` 无防重;`schemas()` **依赖 `Vec` 插入顺序**(change A 有测试断言 `schemas[0]/[1]` 顺序)。
- `ToolContext { cwd, max_output_bytes }`、`ToolOutcome { content, is_error, truncated }` 已就位;Agent loop 的 `gate` 已保证 `Deny` 不调用 `execute`。
- `Cargo.toml`:无 `ignore`/`globset`/`regex`,`tokio` 仅 `rt-multi-thread,macros`。

设计依据:§5.3(Tool 表)、§5.4(权限)、§10(测试 + 边界用例)、§11(crate 选型)。属强制 TDD。

## Goals / Non-Goals

**Goals:** 实装 7 个工具并兑现契约(截断 / 唯一匹配 / shell 捕获+超时);registry 防重名;全部 `tempfile` tempdir TDD(正常+失败+边界);变更工具拒绝路径经 loop 验证无副作用。

**Non-Goals(留后续):** live HTTP/SSE/凭据(transport)、Anthropic、TUI、配置分层;`run_shell` 权限 diff 预览(§5.4 `PermissionPreview` → TUI change);`main` 接 Loop(→ transport change,见 DB9)。

## 工具契约(本轮 review 用)

| 工具 | 权限 | args | 行为 | 失败 / 边界 |
|---|---|---|---|---|
| `list_dir` | ReadOnly | `{path?}` | `ignore` 列目录(默认 cwd),gitignore 感知 | 不存在 → is_error |
| `read_file` | ReadOnly | `{path, offset?, limit?}` | 读取 + 按**行** offset/limit 分页 + **字节**截断 | 不存在 → is_error;超 `max_output_bytes`(UTF-8 边界)→ truncated |
| `glob` | ReadOnly | `{pattern, path?}` | **`ignore` 遍历 + `globset` 过滤**(globset 仅 matcher,不枚举) | 非法 pattern → is_error |
| `grep` | ReadOnly | `{pattern, path?}` | `ignore` 遍历 + `regex` 搜索,返回匹配行 | 非法 regex → is_error;超限(UTF-8 边界)→ truncated |
| `write_file` | RequiresConfirmation | `{path, content}` | 新建 / 覆盖写入(父目录**不**自动创建) | 写失败 / 父目录不存在 → is_error |
| `edit_file` | RequiresConfirmation | `{path, old_string, new_string}` | str-replace,唯一匹配 | 0 / 多匹配 → is_error 且不写 |
| `run_shell` | RequiresConfirmation | `{command, timeout_secs?}` | 平台 shell 捕获 stdout/stderr/exit + timeout + 截断 | 超时 / 非零 exit → is_error;超限(UTF-8 边界)→ truncated |

## Decisions

- **DB1 工具落 3 文件(§4)**:`tool/fs.rs`(list_dir/read_file/glob/grep)、`tool/edit.rs`(write_file/edit_file)、`tool/shell.rs`(run_shell);`tool/mod.rs` 加 `pub mod fs/edit/shell;`。
- **DB2 防重名:保留 `Vec` + `register` 改返回 `Result`。** 你给的两选项中选「注册防重名」而非换 `HashMap`——因 `schemas()` 依赖插入顺序(change A 测试 + 模型侧工具顺序确定性),`HashMap` 不保序、保序需引 `indexmap`(新依赖,不值)。`register(&mut self, tool) -> Result<(), ToolRegistryError>`,重名 → `Err(Duplicate(name))` 不覆盖。`get` 仍 O(n)(n≤7,可忽略)。代价:既有 `register(...)` 调用点补 `.unwrap()`。
- **DB3 依赖**:`ignore`(gitignore 感知遍历,list_dir/grep/glob)、`globset`(glob 过滤)、`regex`(grep);`tokio` += `process`(跑命令)+ **`time`**(`run_shell` 的 `tokio::time::timeout` 必需——超出你提的 `process`,显式补);dev-dep `tempfile`。均见 §11。
- **DB4 工具自吞错误**:`execute` 返回 `ToolOutcome`(非 `Result`),故每个工具 catch 自身 IO/解析错误 → `ToolOutcome{is_error:true, content:<msg>}`;args 从 `Value` 防御式解析,缺字段/类型错 → is_error。
- **DB5 截断(UTF-8 安全)**:仅 `read_file`/`grep`/`run_shell` 按 `ctx.max_output_bytes` 截断置 `truncated`(§5.3 要求项);`list_dir`/`glob`/`write_file`/`edit_file` 不要求截断。**截断取 ≤ `max_output_bytes` 的最近 UTF-8 字符边界**(不可裸 `&s[..n]`,否则跨多字节字符 panic / 产生非法 String;用 `floor_char_boundary` 思路或 `char_indices` 求边界)。
- **DB6 edit_file 唯一匹配**:`old_string` 必须恰好出现一次;`matches().count()` 为 0 或 >1 → is_error 且不写(§5.3 / §10 边界)。
- **DB7 run_shell 跨平台 + 超时 + content 格式**:平台 shell 经 `cfg`(Windows `cmd /C`、Unix `sh -c`);`tokio::process::Command` 捕获 stdout/stderr/exit;`tokio::time::timeout` 包裹,超时须**真杀子进程**——`Command::kill_on_drop(true)` 或显式 `child.kill()`,否则 timeout 只丢弃 future、tokio `Child` 默认不 kill → **孤儿进程**;超时 → is_error。**content 格式固定**(如 `exit: <code>` + 分段 `--- stdout ---\n…\n--- stderr ---\n…`),供模型解析。**本仓库 win32**,测试用 `cfg` 分支命令(`cmd /C exit 1`;hang 用 `ping -n` 而非 `timeout /t`——后者在重定向下报错)。
- **DB8 write_file 无 diff 预览**:§5.4 的 `PermissionPreview`/`build_preview`(写时显示 diff)属权限 UI,留 TUI change;本 change 工具只写,不生成预览。
- **DB9 `main` 不接 Loop(沿用你的倾向)**:无真 provider 时 `main`+Loop+Mock 价值有限,且需 stdin y/n decider + 拆 `src/lib.rs` 起 `tests/` 端到端。留 transport change。本 change `main` 仍单轮;工具测试一律 in-crate `#[cfg(test)]` + tempdir,**不需 `lib.rs`**。
- **DB10 拒绝-无副作用属 characterization**:`gate` 已保证 `Deny` 不调 `execute`(change A 已测 mock 工具),故「实体变更工具被拒 → 文件未建」这组测试**预期直接通过**(非红绿),目的是验证真实工具确受门约束。按 CLAUDE.md 不伪造红灯,明确标为 characterization。脚本须 `[变更工具 tool_call, 最终文本]` 两条,否则 deny→continue→再请求会撞脚本耗尽变 `AgentError::Provider`。
- **DB11 停点策略**:`Tool` trait 已在 change A 确认;7 工具共用该 trait + 同一实现模式。全部工具 args/行为契约已在上表列出供本轮 review,故 apply 时仅设 2 个实现停点——**首个 ReadOnly 工具**(`read_file`,确立 arg 解析/cwd/截断/错误模式)与**首个 RequiresConfirmation 工具**(`write_file`,确立变更模式);其余沿用已确认模式不停(补边界 case 连写)。若你想每个工具都停,apply 时说一声。
- **DB12 ignore 测试确定性**:`list_dir`/`grep`/`glob` 的 `ignore` walker 默认读全局 gitignore;tempdir 测试须禁全局 gitignore(`WalkBuilder::git_global(false)` 等)或只断言非忽略文件,避免环境干扰。`tempfile::tempdir()` 落系统临时区,不受 repo `.gitignore` 影响。
- **DB13 ReadOnly 路径范围风险(1.0 接受)**:`read_file`/`list_dir`/`glob`/`grep` 经 `resolve_path` 不限定 `ToolContext.cwd`:绝对路径直用,相对路径可经 `..` 逃逸。且 ReadOnly 工具自动执行、无权限门,因此模型或经被读文件的 prompt injection 可无确认读取任意用户可读文件(如 `~/.ssh`、`.env`),内容进入 LLM context。1.0 接受该风险(agent 代表用户执行本地读操作);收口点放到技术方案 §13 的 1.3 `PolicyEngine`(读路径策略/限定)。本 change 只记录风险,不在工具层加路径限定。

## Risks / Trade-offs

- **[Vec O(n) 查找]** n≤7,可忽略;保序收益 > 查找成本,故不换 `HashMap`/`indexmap`。
- **[run_shell 跨平台 + 超时 kill]** → `cfg` 选 shell;`kill_on_drop(true)`/显式 kill 防孤儿;Windows 测试用 `cmd` 分支,避免不可移植命令。
- **[字节截断跨 UTF-8 字符]** → 取字符边界,见 DB5;否则 panic / 非法 String。
- **[ReadOnly 路径过宽]** → 1.0 记录并接受,见 DB13;后续由 `PolicyEngine` 收口读路径策略。
- **[register 签名变更波及 change A]** → 机械补 `.unwrap()`;`schemas()` 顺序不受影响(仍 `Vec`),change A 顺序断言测试保持绿。
- **[`tokio` 加 `time` 超出你提的 `process`]** → `timeout` 必需,已在 DB3/proposal 显式记。

## Migration Plan

纯加法(3 文件 + 3 依赖 + tokio feature)+ `register` 签名机械更新。无数据迁移。回滚 = revert 本 change 提交(撤依赖、撤 register Result、撤 3 工具文件)。

## Open Questions

- `main` 接 Loop 的时点 → 定在 transport change(有真 provider 后)。
- `run_shell` 命令解析:1.0 走 shell `-c`/`/C`(单字符串命令,§5.3 语义);argv 形式留后续若有需。
