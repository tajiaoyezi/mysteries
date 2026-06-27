# 2026-06-27 · 14 · archive finish-1-0 🎉 1.0 feature-complete

## 决策

- **单 change 收 §12 step5 三块**(内置命令 / 超时接线 / ToolOutcome.exit)→ **1.0 feature-complete** | 主导:用户定单 change(主 agent 建议拆 2,用户拍板 1;ToolOutcome.exit 列为 1.0 最后任务)| 依据:§8 / §12 step5
- **D1 6 delta 全 ADDED**:builtin-commands NEW + agent-loop/tui-shell/tool-system/builtin-tools/cli-runtime 加性;provider-abstraction 不动、openai/anthropic-transport 无 spec 变(超时值来源=构造器注入,impl 细节)
- **D2 命令解析/执行分离**:`parse_command(&str)->Option<Command>`(纯 red-green)+ 执行;on_key 提交分流
- **D3 /status 零 agent 改动**:AppState 持 session 快照;`msgs`=对话块(User/Assistant)计数;`iter`=UI 计 CallingModel、每轮 Prompt/TurnComplete 重置(=当前轮次,配 max 出 X/N)
- **D4 /model 全切换**:`Agent::set_model`(加性,run/run_observed 零回归)+ `UserInput::SetModel` 经 channel,run_agent_task 轮间生效(下一轮)| 红灯停点
- **D5 timeout 注入**:provider 暴露 `with_attempt_timeout` 构造器,`select_provider` 据 `config.timeout_secs` 注入 RetryPolicy → 收口此前 dangling 的 config.timeout_secs;transport per-attempt 行为不变 → 无 spec 变
- **D6 `ToolOutcome.exit: Option<i32>` behavior-preserving**:run_shell 设 `status.code()`(signal→None);C5 foot 仅 Some 渲;其余工具机械 += `exit:None`(含 permission MockTool 等测试 mock,纯 ripple);**run_shell content 的 `exit:` 文本保留**(模型靠 content 知道精确码,`ToolOutcome.exit` 仅喂 UI);C 殿后(加字段牵动最广)
- **停点**:set_model 红灯(③,动 agent-loop)+ C8/C9/run_shell exit-foot 三帧对眼(⑤/⑥)
- **审查修正(对眼打回 2 处)**:① `msgs` 原数 transcript 全部块 → /status 快照块自计、同帧 1 vs 2 矛盾 → 改只数 User/Assistant 对话块;② run_shell 卡 exit 显两次(body content 首行 + foot)→ 卡 body 渲染滤首行 `exit:`(content 不动、模型仍得码)| 主 agent 对眼
- **里程碑**:§12 五步全完成 —— 双 provider(OpenAI+Anthropic)+ Agent Loop + 7 工具 + 权限门 + 配置/凭据 + ratatui 双主题完整 TUI(工具卡/phase/diff/C7/滚动/spinner/C8/C9)+ 7 内置命令。**1.0 feature-complete。**

## 变更

- 新增 `src/tui/command.rs` + 3 快照(help / status / run_shell_exit_foot);改 `agent/mod.rs`(set_model)、`app.rs`(timeout 注入)、`permission/mod.rs`(exit ripple)、`provider/{mod,openai,anthropic}`(with_attempt_timeout)、`tool/{mod,edit,fs,shell}`(ToolOutcome.exit + run_shell)、`tui/{app,channel,mod,render}`(命令/SetModel/C8/C9/meta/exit foot/msgs/body 滤)
- 验证:`cargo test` 149 passed / 2 ignored;`clippy --all-targets` 零警告;`fmt` 通过;**零新依赖**(`Cargo` 无 diff);零回归(`agent::` 12 / `tool::` 27)
- archive:`changes/finish-1-0` → `changes/archive/2026-06-27-finish-1-0`;`specs/` builtin-commands NEW(3)+ agent-loop/builtin-tools/cli-runtime/tool-system/tui-shell 共 +11 delta
- 注:openspec「>10 deltas 考虑拆分」非阻断提示 —— 用户定的 finale 单 change,已知 grab-bag 宽度,非 capability 冲突

## 待决(post-1.0 / 1.x,非 1.0 范围)

- **§13 1.x 路线**:1.1 token 压缩(`ContextStrategy`)、1.2 持久化(`SessionStore`)、1.3 PolicyEngine(读路径 confinement,DB13)、1.4 TUI 体验(256/16 降级 / markdown / 运行时主题切换 config·/theme / 滚动行级·Home·End / 输入历史 / spinner 空闲暂停)、1.5 并行工具、2.0 MCP·subagent、OAuth
- 重试次数可配、run_shell content `exit:` 文本是否随结构化精简、§5.1 `tool_mode` 本地非 function-calling 降级、C7 provider 特定 title(需 `AgentEvent::Error` 携结构)
- 旧 `leafiellune` 身份仍在 `refs/original` + reflog(git filter-branch 改 wanglei30 后未清,待用户确认是否 purge)

## 引用

- change:`finish-1-0`(rationale / rejected alternatives 全量见 design.md D1–D7;archive 路径 `changes/archive/2026-06-27-finish-1-0`)
- 技术方案 §8 / §12 step5(完成)/ §13(1.x 扩展缝)
- `设计规范/03` C5/C8/C9/C10
- 前置 change:`add-anthropic-provider`(13)、`add-tui-polish`(12)、`add-config-layering`(07,config.timeout_secs)
- session log:无专属 checkpoint —— 子 agent propose + implement(停点 set_model 红灯 + C8/C9/exit-foot 对眼);主 agent review(set_model 加性零回归、ToolOutcome.exit 机械 ripple、timeout 注入、对眼打回 msgs 计数 + run_shell exit 重复)+ commit / archive
- **🎉 1.0 feature-complete:14 个 change、13 能力 spec、149 passed / 2 ignored、clippy 零警告、零无谓依赖**
