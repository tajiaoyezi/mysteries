## 1. 命令解析(NEW builtin-commands · TDD)

- [x] 1.1 【红】写 `parse_command` 测试:`/help`/`/clear`/`/model`/`/model x`/`/status`/`/exit`/`/login`/`/logout`/`/xyz` → 对应 `Command`;非 `/` → `None`;确认失败
- [x] 1.2 【绿】实现 `Command` enum + `parse_command(&str) -> Option<Command>`(纯,见 design D2)
- [x] 1.3 【重构】清理

## 2. AppState session 快照 + iter + 命令执行(TDD)

- [x] 2.1 【红】写测试:`AppState` 持 session 快照(provider/model/maxIter/cwd/tools);`apply(StatusChanged(CallingModel))` → iter+1、新 prompt/`TurnComplete` 重置;`/clear` 清 transcript、`/help`→C8 块、`/status`→C9 块、`/login`·`/logout`·`Unknown`→notice;确认失败
- [x] 2.2 【绿】`AppState` 加 session 快照 + iter 计数 + `TranscriptBlock::{Help,Status,Notice}` + 命令执行(`on_key` 提交 `parse_command` 分流,见 design D2/D3)
- [x] 2.3 【重构】清理

## 3. /model 切换(强制 TDD · 红灯停点 · 动 agent-loop)

- [x] 3.1 【红 · 停点】写测试:`Agent::set_model("m2")` 后跑一轮(Mock)→ `ModelRequest.model=="m2"`,其余循环行为不变(既有 agent-loop 测试保持绿);`run_agent_task` 收 `UserInput::SetModel` → 下一轮 model 改;`/model`(无参)→ notice 显当前、`/model x` → UI 更新 + 发 SetModel;确认失败。**贴 `set_model` / `SetModel` 草案 + 失败输出,停下等确认**(agent-loop 新方法 / 切换接口首次成型)
- [x] 3.2 【绿】`Agent::set_model(&mut self, String)`(agent-loop);`UserInput::SetModel` 变体;`run_agent_task` 加 `SetModel` arm(`mut agent` + `set_model`);`AppState` /model 执行(见 design D4)
- [x] 3.3 【验收】既有 agent-loop 全部测试保持绿(零回归)
- [x] 3.4 【重构】清理

## 4. 超时接线(MODIFIED cli-runtime · TDD)

- [x] 4.1 【红】写测试:`config.timeout_secs = 12` → `select_provider`(OpenAi / Anthropic)所得 provider 的 `RetryPolicy.attempt_timeout == 12s`,构造不触网;确认失败
- [x] 4.2 【绿】openai/anthropic 暴露 timeout-taking pub 构造器;`select_provider` 据 `config.timeout_secs` 注入 `RetryPolicy`(见 design D5;openai/anthropic-transport 无 spec 变更)
- [x] 4.3 【重构】清理

## 5. 命令块 + 状态行 meta 渲染(带色 insta · 对眼停点)

- [x] 5.1 【绿】`render`:C8 帮助块(两列 7 命令)、C9 快照块(provider·model·iter·msgs·cwd·tools)、notice 块、状态行右侧常驻 meta(`设计规范/02·03`,复用 Theme/`buffer_to_styled`)
- [x] 5.2 【insta · 停点】带色快照:C8 帮助块、C9 快照块、状态行 meta;`cargo insta review` —— **C8 / C9 首帧 themed 人工对眼**(对 `原型截图` C8/C9 区域)。**贴帧给用户审**

## 6. ToolOutcome.exit + run_shell + C5 foot(最后一组 · TDD + insta + 对眼)

- [x] 6.1 【红】写测试:`ToolOutcome.exit` 默认 `None`;`run_shell` 退出码 0 → `exit==Some(0)`、content/is_error 不变;其余 6 工具 `exit==None`;确认失败
- [x] 6.2 【绿】`ToolOutcome` 加 `exit: Option<i32>`(机械更新所有字面量 += `exit: None`,behavior-preserving);`run_shell` 设 `output.status.code()`(保留 content 文本);`ToolCard` 加 `exit`,`apply(ToolCallFinished)` 填 `outcome.exit`(见 design D6)
- [x] 6.3 【验收】既有工具 / agent / app 测试全绿(加字段 behavior-preserving,既有断言更新但语义不变)
- [x] 6.4 【绿+insta · 对眼停点】`render` C5 exit foot 仅 `Some` 渲(非 0→`error.fg`),`None` 不渲;带色快照 run_shell 卡(含 foot)+ 既有工具卡(无 foot、零回归);**run_shell 卡 exit foot 首帧 themed 对眼**。**贴帧给用户审**

## 7. 收尾

- [x] 7.1 `cargo build`、`cargo test` 默认全绿(命令/iter/set_model/超时/exit 全 red-green + 带色 insta,无终端 / 不触网)、`cargo fmt`;`cargo run` 进 TUI 冒烟(`/help`·`/status`·`/clear`·`/exit`·`/model`)
- [x] 7.2 自检:6 个 delta(builtin-commands NEW + tui-shell/agent-loop/tool-system/builtin-tools/cli-runtime ADDED)requirements 全有落点(red-green / insta / 对眼 已分类);**既有 agent-loop / 工具卡快照零回归**已验;停点(set_model 红灯 + C8/C9/exit-foot 三对眼)已过;偏离已标注(/model 下一轮生效、超时无 transport spec 变、content 保留 exit 文本)。**1.0 feature-complete** 自检
