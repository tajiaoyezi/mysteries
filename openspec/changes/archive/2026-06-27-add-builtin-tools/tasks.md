## 1. 依赖与模块骨架

- [x] 1.1 `Cargo.toml` 加 deps:`ignore`、`globset`、`regex`;`tokio` features 加 `process` + `time`(见 design DB3);dev-dependency 加 `tempfile`
- [x] 1.2 新建 `src/tool/fs.rs`、`src/tool/edit.rs`、`src/tool/shell.rs`;`tool/mod.rs` 加 `pub mod fs; pub mod edit; pub mod shell;`;骨架 `cargo build` 通过

## 2. ToolRegistry 防重名(refactor + TDD)

- [x] 2.1 `register` 改返回 `Result<(), ToolRegistryError>`(暂总是 `Ok`)+ 定义 `ToolRegistryError::Duplicate`;更新既有调用点补 `.unwrap()`(change A 的 `tool`/`agent` 测试);`cargo build` + 既有 `cargo test` 仍绿(refactor,行为不变)
- [x] 2.2 【红】写「重名注册 → `Err(Duplicate)` 且不覆盖原工具;唯一名 → `Ok`」测试,确认失败(当前总是 `Ok`)
- [x] 2.3 【绿】`register` 加重名检测 → `Err`
- [x] 2.4 【重构】清理

## 3. read_file(首个只读工具,强制 TDD · 停点①)

- [x] 3.1 【红 · 停点】`tempfile` tempdir 测试:读取内容、按**行** `offset`/`limit` 分页、输出超 `max_output_bytes` → `truncated`、路径不存在 → is_error;确认失败;**贴出 `read_file` schema/args 契约 + 失败输出,停下等确认**(首个实体工具,确立 arg 解析 / cwd 解析 / 截断 / 错误编码模式)
- [x] 3.2 【绿】实现 `read_file`(行 `offset`/`limit`;字节截断取 **UTF-8 字符边界**,见 design DB5)
- [x] 3.3 【重构】清理

## 4. list_dir / glob / grep(只读,沿用已确认模式不停)

- [x] 4.1 【红】`list_dir` tempdir 测试:gitignore 感知列目录、路径不存在 → is_error;确认失败(walker 禁全局 gitignore 保确定,见 DB12)
- [x] 4.2 【绿】实现 `list_dir`(`ignore`)
- [x] 4.3 【红】`glob` 测试:匹配 tempdir 内若干文件、非法 pattern → is_error;确认失败
- [x] 4.4 【绿】实现 `glob`(**`ignore` 遍历 + `globset` 过滤**——globset 仅 matcher 不枚举,见 DB 契约表)
- [x] 4.5 【红】`grep` 测试:正则匹配(含定位)、非法正则 → is_error、输出超限 → `truncated`;确认失败
- [x] 4.6 【绿】实现 `grep`(`ignore` + `regex`;字节截断取 UTF-8 边界)
- [x] 4.7 【重构】清理

## 5. write_file(首个变更工具,强制 TDD · 停点②)

- [x] 5.1 【红 · 停点】tempdir 测试:新建写入、覆盖既有、父目录不存在 → is_error(不自动建)、写失败 → is_error;确认失败;**贴出 `write_file` schema/args 契约 + 失败输出,停下等确认**(首个变更工具,确立变更模式)
- [x] 5.2 【绿】实现 `write_file`
- [x] 5.3 【重构】清理

## 6. edit_file / run_shell(变更,强制 TDD)

- [x] 6.1 【红】`edit_file` 测试:唯一匹配替换、0 / 多匹配 → is_error 且文件未改;确认失败
- [x] 6.2 【绿】实现 `edit_file`
- [x] 6.3 【红】`run_shell` 测试:捕获 stdout/stderr/exit、超时 → is_error、非零退出 → is_error、输出超限 → `truncated`;平台命令用 `cfg` 分支(Windows `cmd /C exit 1`、hang 用 `ping -n` 而非 `timeout /t`,见 DB7);确认失败
- [x] 6.4 【绿】实现 `run_shell`(`tokio::process` + 平台 shell;`tokio::time::timeout` + **`kill_on_drop(true)`/显式 kill** 防孤儿;content 按固定格式拼 exit/stdout/stderr;字节截断取 UTF-8 边界;见 DB7)
- [x] 6.5 【重构】清理

## 7. 变更工具拒绝无副作用(characterization,非红绿)

- [x] 7.1 经 Agent loop + tempdir + 注入 `DenyAll`:`write_file`(至少代表一个变更工具)被拒 → 目标文件未建、history 含 is_error `ToolResult`;**脚本须 `[write_file tool_call, 最终文本]` 两条**(否则撞脚本耗尽变 `AgentError::Provider`);**预期直接通过**(gate 已保证 deny 不执行,见 design DB10),作真实工具受门约束的回归验证

## 8. 收尾

- [x] 8.1 `cargo build` 通过、`cargo test` 全绿、`cargo fmt`(可选 `cargo clippy`)
- [x] 8.2 自检:`builtin-tools` 7 工具 + 拒绝无副作用 + `tool-system` 重名拒绝 的 spec requirements 全有测试落点;§10 边界(`edit_file` 非唯一、`truncated`、`run_shell` 超时)覆盖;`ignore`/`globset`/`regex` + `tokio` `time` 已记;`main` 仍单轮(接 Loop 留 transport change)
