## 1. 抽象类型与字段(契约,纯类型不走 red-green)

- [x] 1.1 新建 `src/tool/mod.rs`、`src/permission/mod.rs`;`src/main.rs` 加 `mod tool; mod permission;`;骨架 `cargo build` 通过
- [x] 1.2 定义类型:`ToolOutcome{content,is_error,truncated}`、`ToolContext{cwd:PathBuf,max_output_bytes:usize}`、`PermissionLevel{ReadOnly,RequiresConfirmation}`(`tool/mod.rs`);`PermissionDecision{Allow,Deny}`(`permission/mod.rs`);按需 derive `Debug`/`PartialEq`/`Clone`
- [x] 1.3 `provider/mod.rs` 加 `ToolSchema{name,description,parameters:Value}` + 给 `ModelRequest` 加 `tools: Vec<ToolSchema>`;更新所有既有 `ModelRequest` 字面量补 `tools: Vec::new()`(`wire`/`mock`/`provider` 测试、`run_single_turn`);`cargo build` + 既有 `cargo test` 仍绿
- [x] 1.4 `agent/message.rs` 给 `Message` 加 `Clone` derive(见 design DA2);`cargo build` 通过
- [x] 1.5 `error.rs` 加 `AgentError`(`thiserror`:`Provider(#[from] ProviderError)`、`MaxIterations{limit:u32}`,见 design DA3)

## 2. provider-abstraction delta:wire tools 序列化(强制 TDD)

- [x] 2.1 【红】写 wire 测试:带 2 个 `ToolSchema` 的请求 → `body.tools` 数组(各含 `function.name`/`description`/`parameters`);无工具 → 无 `tools` 键;确认失败
- [x] 2.2 【绿】扩展 `serialize_request`,在 `!tools.is_empty()` 时输出 OpenAI `tools` 数组
- [x] 2.3 【重构】保持绿

## 3. Tool trait + ToolRegistry(强制 TDD · 停点)

- [x] 3.1 【红 · 停点】写 Tool/Registry 契约测试:in-test mock `Tool`(一个 `ReadOnly`、一个 `RequiresConfirmation`);注册、按名查找(命中 / 未命中 → None)、`execute` → `ToolOutcome`、`registry.schemas()` 返回各项 `name`/`description`/`parameters`;确认失败;**贴出 `Tool` trait + `ToolRegistry` 草案 + 失败输出,停下等确认**(新 trait 首次成型)
- [x] 3.2 【绿】定义 `Tool`(`#[async_trait]`)+ `ToolRegistry`(register / get / schemas),最小实现让 3.1 过
- [x] 3.3 【重构】清理

## 4. PermissionDecider seam + gate(强制 TDD · 停点)

- [x] 4.1 【红 · 停点】写权限门测试:`ReadOnly` → `Allow` 不询问 decider;`RequiresConfirmation` + 注入 AllowAll → decider 被调用返回 `Allow`;注入 DenyAll → `Deny`;确认失败;**贴出 `PermissionDecider` trait + `gate` 签名 + 失败输出,停下等确认**(新权限路径)
- [x] 4.2 【绿】定义 `PermissionDecider`(`#[async_trait]`)、`PermissionDecision`、`gate(call, tool, decider)`;提供 in-test AllowAll / DenyAll decider;最小实现让 4.1 过
- [x] 4.3 【重构】清理

## 5. Agent Loop 核心(强制 TDD,§10 范围 1/2/3)

- [x] 5.1 【红】§10-1 主循环:MockProvider 脚本 [无 tool_call 文本] → 单轮终止返回文本、history 末为 `Assistant`;脚本 [tool_call, 文本] → `Assistant{tool_calls}` + `ToolResult` + 再请求(第二次请求带累积 history)+ 终止;断言 history 顺序与 `recorded_requests()` 增长;确认失败
- [x] 5.2 【绿】实现 `Agent` 结构 + `run`(Allow 路径:registry 分发 + gate + `execute` + `ToolResult` 回填 + 终止判定),让 5.1 过
- [x] 5.3 【红】§10-2/3 + 边界:工具失败 → is_error `ToolResult` 续、未知工具 → is_error 续、注入 DenyAll 拒绝 → is_error 续(§10-3)、`max_iterations` 触顶 → `AgentError::MaxIterations`、provider 错误 → `AgentError::Provider`;确认失败
- [x] 5.4 【绿】扩展 `run` 覆盖以上可恢复 / 致命分支(gate 接 decider、未知工具、守卫、错误分流)
- [x] 5.5 【重构】保持绿,清理

## 6. 收尾

- [x] 6.1 `cargo build` 通过、`cargo test` 全绿、`cargo fmt`(可选 `cargo clippy`)
- [x] 6.2 自检:`agent-loop` / `tool-system` / `permission-gate` / `provider-abstraction`(ADDED)四个 spec 的 requirements 全有测试落点;§10 范围 1/2/3 覆盖;D5 反转(tools 下发)落地;零新依赖确认;`main` 仍单轮(Loop 改接属 change B)
