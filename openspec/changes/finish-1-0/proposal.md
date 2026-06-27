## Why

Anthropic 落地后,§12 step5 仅剩三块收尾,做完即 **1.0 feature-complete**:① 内置命令(§8 C8/C9,目前输入框不识别 `/`);② 超时接线(`config.timeout_secs` 已 resolved 却没被 provider 用 —— openai/anthropic 硬编码 30s);③ `ToolOutcome.exit`(工具卡 C5 的 exit foot 缺数据源,run_shell 退出码现仅塞在 content 文本里)。用户已定**单 change**收掉。本 change 不开 1.x 路线(token 压缩/持久化/PolicyEngine/MCP/subagent,§13)。

## What Changes

**A. 内置命令(NEW `builtin-commands`)** —— `parse_command(&str) -> Command`(`/` 前缀纯解析):
- `/clear` 清 transcript;`/help` → C8 帮助块(7 命令两列表);`/status` → C9 快照块(provider·model·iter·msgs·cwd·tools=7);`/exit` 退 TUI;`/login`·`/logout` → 占位 notice;`/model` 见下;未知 `/x` → notice。
- **数据源**:provider/model/maxIter/cwd/tools 由 `run_tui` 装配时把 session 快照传入 `AppState`;`msgs` = transcript 块数;`iter` = UI 计当前轮 `StatusChanged(CallingModel)` 次数(零 agent 改动)。

**B. 超时接线(MODIFIED `cli-runtime`)** —— `select_provider` 用 `config.timeout_secs` 构造 provider 的 per-attempt 超时(经新 pub 构造器注入 `RetryPolicy`);openai/anthropic 的 `with_retry_policy` 现私有 → 暴露 timeout-taking 构造器。重试次数沿用默认常量(可顺带配,留默认)。

**C.(最后一组 task)`ToolOutcome.exit`(MODIFIED tool-system / builtin-tools / tui-shell)** —— `ToolOutcome` 增 `exit: Option<i32>`;`run_shell` 设 `output.status.code()`(本就是 `Option<i32>`),其余 6 工具 `None`;`ToolCard` 增 `exit`,C5 **exit foot 仅 `Some` 时渲**(其余工具 `None` 不渲 → **既有工具卡快照零回归**)。

### 5 点定夺

1. **capability 划分** → NEW `builtin-commands`;MODIFIED `tui-shell`(命令块渲染 + 状态行 meta + exit foot + `UserInput::SetModel`)、`agent-loop`(`set_model`,仅 /model 切换)、`tool-system`(`ToolOutcome.exit`)、`builtin-tools`(run_shell exit)、`cli-runtime`(超时注入)。**6 个 delta 全 ADDED**(纯加性、新关注点,零回归);`provider-abstraction` 不动;openai/anthropic-transport **无 spec 变更**(超时仍「per-attempt timeout」,仅值来源变 = 构造器注入,impl 细节)。
2. **命令模型** → `on_key` 提交时 `/` 前缀 → `parse_command`(纯 red-green)→ 执行(状态变更）。各命令语义见上;`/status` 数据源见上。
3. **/model 运行时切换** → **全切换**:`/model <name>` → UI 乐观更新 model 快照 + 发 `UserInput::SetModel(name)` → `run_agent_task` 对 idle agent 调 `Agent::set_model`(**下一轮生效**);`/model` 无参 → notice 显当前 model。**capability 影响:MODIFIED agent-loop(`set_model` 新方法)+ tui-shell(`SetModel` 变体)**,均 additive、既有 `run`/`run_observed` 零回归。**(备选 not-yet:仅显示、`<name>` 标暂不支持 → 省掉 agent-loop delta;但 §8 明列「切换」,且你已预批该处停点 → 取全切换。)**
4. **B 接线** → provider 吃 `config.timeout_secs`:`select_provider` 构造 `RetryPolicy{attempt_timeout: from_secs(config.timeout_secs), max_retries: 默认, backoff: 默认}` 注入新构造器;OpenAi/Anthropic arm 各传。openai/anthropic-transport **行为不变**(per-attempt timeout 依旧,只是值从 config 来)→ **无 spec 变更**,MODIFIED 落在 cli-runtime。
5. **C `ToolOutcome.exit` 形状** → `exit: Option<i32>`;`run_shell` = `output.status.code()`;其余 `None`;C5 foot 仅 `Some` 渲。加字段会动所有 `ToolOutcome` 字面量(机械 += `exit: None`)+ 既有相等断言,**behavior-preserving**(exit 默认 None)。

**停点清单**:
- 【红灯】`Agent::set_model`(agent-loop 新方法,/model 切换接口首次成型)—— 贴草案 + 失败输出等确认。
- 【对眼】C8 帮助块首帧、C9 快照块首帧、run_shell 工具卡含 exit foot 首帧 —— 三处 themed `insta` 人工对眼(`原型截图` C8/C9 区域 + C5 foot;`设计规范/README` 关卡)。

**port/adapt/drop(UI 部分)**:port ✅ = C8 两列命令表、C9 快照字段、C5 exit foot `exit {code}`、状态行 meta(`02`/C9/C5);adapt ⚠️ = 圆角→box、非 0 exit→`error.fg`;drop ❌ = 无新动画。

**明确不含**:1.x 路线(§13:token 压缩/持久化/PolicyEngine/并行/MCP/subagent)、256/16 降级(1.4)、运行时主题切换。

## Capabilities

### New Capabilities

- `builtin-commands`: slash 命令解析与执行 —— `/help` `/clear` `/model [name]` `/status` `/exit` `/login` `/logout`(后二占位);纯解析(red-green)+ 执行(清屏 / 帮助块 / 快照块 / 退出 / 模型查看·切换 / 占位 notice)。

### Modified Capabilities

- `tui-shell`: ADDED —— C8 帮助块 / C9 快照块 / notice 块渲染、状态行常驻 meta(provider·model·iter·msgs·cwd)、工具卡 C5 exit foot、`UserInput::SetModel` 变体。
- `agent-loop`: ADDED —— `Agent::set_model`(运行时模型切换;既有 `run`/`run_observed` 不变)。
- `tool-system`: ADDED —— `ToolOutcome.exit: Option<i32>`(进程类工具设退出码,余 None;既有字段不变)。
- `builtin-tools`: ADDED —— `run_shell` 设 `exit` 为进程退出码(其余 6 工具 None)。
- `cli-runtime`: ADDED —— `select_provider` 用 `config.timeout_secs` 注入 provider per-attempt 超时。

## Impact

- **改动代码**:`tui/{app,render,channel,mod}.rs`(命令解析/执行 + 块渲染 + 状态 meta + exit foot + SetModel + session 快照)、`agent/mod.rs`(`set_model`)、`tool/mod.rs`(`ToolOutcome.exit`)、`tool/shell.rs`(exit)、`provider/{openai,anthropic}.rs`(pub timeout 构造器)、`app.rs`(select_provider 注入超时);大量 `ToolOutcome` 字面量 += `exit: None`(机械)。
- **新增依赖**:**无**。
- **构建 / 测试**:命令解析 / 执行状态 / iter 计数 / 超时注入 / `ToolOutcome.exit` / run_shell exit / `set_model` = **red-green**;C8/C9/exit-foot/状态 meta 渲染 = **带色 insta**;三处 **themed 对眼**。**既有工具卡快照零回归**(exit=None 不渲 foot)、既有 `run` 零回归。`cargo test` 默认全绿、不触网。
- **里程碑**:**1.0 feature-complete**。
