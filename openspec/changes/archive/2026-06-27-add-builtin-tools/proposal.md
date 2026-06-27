## Why

change A 已立起 Agent Loop + 工具系统抽象 + 权限门(`agent-loop` / `tool-system` / `permission-gate` 已在主 specs),但 registry 里**还没有任何实体工具**——Agent 还不能真正读写文件、搜索、执行命令。本 change(技术方案 §12 第 2 步剩余半)实装 §5.3 表中的 7 个实体工具,接到已就位的 `Tool` / `ToolRegistry` / 权限门上,兑现既有契约(截断 / 唯一匹配 / 超时)。按 change A 已定的 2 拆方案,这是「工具」那一批,合为一个 change。

## What Changes

- 新增 **4 个 ReadOnly 工具**(`src/tool/fs.rs`):
  - `list_dir`:`ignore` crate,gitignore 感知列目录。
  - `read_file`:`offset` / `limit` 分页;输出按 `ToolContext.max_output_bytes` 截断并置 `ToolOutcome.truncated`。
  - `glob`:`ignore` 遍历 + `globset` 过滤(globset 仅 matcher,不枚举文件)。
  - `grep`:`ignore` 遍历 + `regex` 内容搜索;结果按 `max_output_bytes` 截断置 `truncated`。
- 新增 **3 个 RequiresConfirmation 工具**:
  - `write_file`(`src/tool/edit.rs`):新建 / 覆盖写入。
  - `edit_file`(`src/tool/edit.rs`):str-replace,**要求唯一匹配**,0 或多匹配 → is_error。
  - `run_shell`(`src/tool/shell.rs`):`tokio::process` 捕获 stdout/stderr/exit + **timeout**;输出截断置 `truncated`。
- **ToolRegistry 防重名**:现为 `Vec` 且 `register` 无防重。改为 `register` 返回 `Result`,重名注册 → `Err`(**保留 `Vec`** 以维持 `schemas()` 插入顺序——见 design DB2)。
- 实体工具的失败一律编码为 `ToolOutcome{is_error:true}`(`Tool::execute` 返回 `ToolOutcome` 非 `Result`)。

**新增依赖**(理由见 §11):`ignore`(gitignore 感知遍历,`list_dir`/`grep`)、`globset`(`glob`)、`regex`(`grep`);`tokio` 加 **`process` + `time`** feature(`process` 跑命令、`time` 供 `run_shell` 的 timeout——`time` 超出你提的 `process`,因 `tokio::time::timeout` 必需,见 design DB3);dev-dependency `tempfile`(tempdir 测试)。

**明确不含**(留后续):live HTTP / SSE / 凭据(transport change)、Anthropic、TUI、配置分层。`run_shell` 的权限 diff 预览(§5.4 `PermissionPreview`)属 TUI change。

**`main` 接 Loop —— 本 change 不做(沿用你的倾向)**:当前无真 provider,`main` + Loop + Mock 仅是低价值 demo,且需引入 stdin y/n decider + 拆 `src/lib.rs` 起端到端测试。留到 transport change(有真 provider 后 `main` 接 Loop 才有意义)。本 change `main` 仍走单轮(change B 的工具用 in-crate `#[cfg(test)]` + tempdir 测试,无需 `lib.rs`)。

## Capabilities

### New Capabilities
- `builtin-tools`: 7 个实体工具(list_dir / read_file / glob / grep / write_file / edit_file / run_shell)及其契约——截断、唯一匹配、shell 捕获 + 超时、变更工具经权限门拒绝时无副作用。

### Modified Capabilities
- `tool-system`: ADDED——`ToolRegistry` 拒绝重名工具注册(`register` 改返回 `Result`);既有注册 / 查找 / schema 行为不变。

## Impact

- **新增代码**:`src/tool/fs.rs`、`src/tool/edit.rs`、`src/tool/shell.rs`;`src/tool/mod.rs` 加 `pub mod fs/edit/shell;` 与 register 防重名。
- **改动既有**:`register` 签名 `()` → `Result<(), ToolRegistryError>`,所有既有 `register(...)` 调用点(change A 的 `tool`/`agent` 测试)补 `.unwrap()`。
- **依赖**:`ignore`、`globset`、`regex`(deps);`tokio` += `process`、`time`;`tempfile`(dev-dep)。
- **前置依赖**:本 change 实现依赖 change A 代码(已在仓库)。
- **测试**:强制 TDD,每个工具一组(正常 + 失败 + 边界:`edit_file` 非唯一匹配、输出超限 `truncated`、`run_shell` 超时),`tempfile` tempdir 驱动(不碰真实 FS 状态);变更工具的拒绝路径经 Agent loop + 注入 `DenyAll` 验证无副作用(characterization——gate 已保证 deny 不执行,见 design DB10)。`run_shell` 测试平台感知(本仓库 win32,见 design DB7)。
- 新 trait 已在 change A 确认;本 change 仅在首个 ReadOnly 工具与首个 RequiresConfirmation 工具处设实现停点(全部工具契约已在 design 列出供本轮 review,见 tasks 说明)。
