# mysteries

自研 terminal Agent CLI(Rust):在终端里对话驱动一个可调用本地工具的 LLM agent,Claude Code 风格交互。**核心能力(Agent Loop、工具系统、权限控制、会话管理)全部自行实现,不依赖任何第三方 Agent SDK / Framework**;协议接入、TUI、HTTP 等用成熟三方库。

```
cargo build --release
mysteries auth login      # 配置 provider + API Key
mysteries                 # 进入 TUI
mysteries --headless "解释一下 src/agent 的结构"   # 无头单轮模式
```

---

## 特性一览

- **Agent Loop 自研**:模型决策 → tool_calls 逐个过权限门 → 执行回传 → 继续推理,支持 `max_iterations` 上限、Esc 即时中断、全事件落 history。
- **7 个内置工具**:`list_dir` / `read_file` / `glob` / `grep` / `write_file` / `edit_file`(唯一匹配替换)/ `run_shell`。
- **三级权限 × 三种模式**:工具按 `ReadOnly / Edit / Execute` 分级;`Normal / AcceptEdits / Yolo` 模式 Shift+Tab 循环切换,写/执行类需内联确认(y/n)。
- **多 Provider**:OpenAI 兼容(含 WPS AI)/ Anthropic / Mock 三类协议,统一 `Provider` trait + registry,`/models` 运行时热切换,流式解析自实现。
- **上下文管理**:`ContextStrategy`(Passthrough / Compacting),超限自动压缩,token 用量与速率实时显示。
- **TUI**(ratatui + crossterm,alt-screen):
  - Assistant 输出 **markdown 渲染**(CommonMark+GFM,syntect 代码语法高亮、简易表格 CJK 对齐);
  - **多行输入**(Ctrl+Enter/Shift+Enter/Ctrl+J 换行,动态框高软换行);
  - **粘贴三件套**:批量 drain 防逐行误提交、跨批续读、≥15 行折叠为 `[Pasted text #N +M lines]` 占位符(提交原样展开);
  - **消息排队**:running 时提交进可见队列,轮次结束自动推进,Esc 两级取消(单按中断+推进 / 快速连按清空);
  - 鼠标拖选复制、滚轮滚动、输入历史 ↑↓、`/` 命令补全、jump-to-bottom。
- **凭据链**:环境变量 → 凭据文件多级查找,敏感信息不落日志。
- **首次运行引导**:未配置时进入 onboarding,`mysteries auth login` 交互式配置。

## 架构总览

| 模块 | 路径 | 职责 |
|------|------|------|
| Agent Loop | `src/agent/` | 核心决策循环:模型调用 → 工具执行 → 结果回传 → 继续推理;Observer 事件外发 |
| Provider | `src/provider/` | `Provider` trait 抽象 + OpenAI/Anthropic/Mock 接入、流式解析、registry 热切换 |
| Tool 系统 | `src/tool/` | `Tool` trait + `ToolRegistry`,7 个内置工具,按 `PermissionLevel` 分级 |
| Permission | `src/permission/` | 权限门:只读放行,写/执行经 `PermissionDecider` 确认;三种模式 |
| Config | `src/config/` | 用户级(`~/.config/mysteries/config.toml`)+ 项目级(`./mysteries.toml`)合并,项目优先 |
| Credential | `src/credential/` | API Key 凭据链(env → 文件),`secrecy` 包裹 |
| TUI | `src/tui/` | ratatui 外壳:双 task + channel 架构,markdown 渲染、输入、排队、选区等 |
| CLI | `src/cli.rs` | `--headless` 无头模式、`auth list/login/logout` |
| 装配 | `src/app.rs` | provider 选择与 agent 组装 |
| Error | `src/error.rs` | `AgentError` / `ProviderError` 错误类型 |

**TUI 运行时形态**:agent task 跑 `Agent::run`,UI task 渲染 + 事件;经 `UserInput` / `AgentEvent` 两条 channel 通信,中断走独立信号。事件循环对 crossterm 事件**批量 drain**(整批单次渲染),是粘贴防误提交与折叠的地基。

## 工程方法

- **OpenSpec 流程**:每个变更 propose(proposal/design/tasks/spec delta)→ apply → archive。已归档 **42 个 change**(`openspec/changes/archive/`),15 个能力域规格沉淀在 `openspec/specs/`(RFC 2119 风格)。
- **TDD**:headless 内核(Loop/工具/权限/Provider 归一化/配置 merge)强制先测后码,红灯独立成步;TUI 外壳走 `TestBackend` + `insta` 快照事后回归。当前 **487 tests 全绿、`clippy -D warnings` 零警告**。
- **决策记录**:`.ai_history/logs/` 共 **42 条**,一 change 一记录——记最终定案、被否决备选(含理由)、open questions;含多轮对抗性复核与 mutation-test 证伪假绿测试的过程。按编号倒序读可快速回溯最近的设计取舍。
- **设计规范**:`设计规范/` 含设计令牌(双主题调色板)、布局交互契约、组件清单与原型截图。

## 配置

| 文件 | 位置 | 说明 |
|------|------|------|
| 用户配置 | `~/.config/mysteries/config.toml` | provider profiles、默认 model 等 |
| 项目配置 | `./mysteries.toml` | 同结构,**项目优先**合并 |
| 凭据 | `~/.config/mysteries/credentials` | `mysteries auth login` 写入;env 变量优先 |

## 键位与命令速查

| 键 | 行为 |
|----|------|
| `Enter` | 提交(粘贴突发批内的 Enter 自动视为换行) |
| `Ctrl+Enter` / `Shift+Enter` / `Ctrl+J` | 插入换行 |
| `↑` / `↓` | 多行内移动光标;首/末行翻输入历史 |
| `Esc` | 关浮层 > 清选区 > 排队两级取消 > 中断本轮 |
| `Shift+Tab` | 权限模式循环(Normal / AcceptEdits / Yolo) |
| `Ctrl+O` | 展开/折叠工具调用卡片 |
| `Ctrl+Home` / `Ctrl+End` | transcript 顶/底 |
| 鼠标拖选 → 松开或 `Ctrl+C` | 复制选区 |

斜杠命令:`/help` `/clear` `/model [name]` `/models` `/status` `/exit`。

## 测试

```
cargo test --lib                          # 全量(487 passed)
cargo clippy --all-targets -- -D warnings # 零警告基线
```

纯逻辑用 Mock Provider / 临时目录,不依赖真实网络;TUI 快照在 `src/tui/snapshots/`。
