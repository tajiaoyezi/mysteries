# add-thinking-mode(思考模式:统一 Depth 抽象 + Anthropic/OpenAI 双链 + /think 命令)

## Why

现在 agent **完全没接思考模式**:`ModelRequest`(provider/mod.rs:34)只有 `model/messages/tools/max_tokens` 四字段,两个 wire 都不发任何 thinking/reasoning 开关,两个 stream 也无 thinking 分支——不是"在想没展示",是根本没开。Claude 当代模型(Opus 4.8/4.7、Sonnet 5)的思考是 **opt-in**,不传 `thinking` 就纯不想;OpenAI reasoning 模型也不带 `reasoning_effort`。用户要:①开关思考、②类 Claude 的 effort 深度调整、③把思考过程在 TUI 展示出来。

**跨模型字段差异极大**(已 web 核实,见 design):Anthropic 当代走 `thinking:{type:"adaptive",display:"summarized"}` + 顶层 `output_config:{effort}`,老模型(Opus 4.5/Haiku 4.5)走 `thinking:{type:"enabled",budget_tokens}`——且当代模型**传 budget_tokens 直接 400**;OpenAI 走顶层 `reasoning_effort` + `max_completion_tokens`。所以不能按 provider 一刀切,要**统一深度抽象 + per-model 能力表 + 各 wire 层映射**。

## What Changes

1. **统一深度抽象 `Depth { Off, Low, Medium, High, Xhigh }`**(provider 无关):唯一进 `config` / `/think` / `ModelRequest` 的表示,各 wire 层各自向下映射。命名对齐 OpenRouter/Anthropic effort。
2. **`ModelRequest` 加 `thinking: Option<ThinkingConfig>`**(内含 `Depth`,并给 `ModelRequest` 补 `#[derive(Default)]`——现无);`ModelResponse` 加 `thinking: Vec<ThinkingBlock>`;`Message::Assistant` 加 `thinking: Vec<ThinkingBlock>`(字段级 `#[serde(default)]` 保旧 session 兼容)——**既给 TUI 展示 text,又携 signature 供下一轮原样回传**(Anthropic 带 tool_use 多轮的硬约束);`DeltaSink` 加 `on_thinking(&str)` 默认空实现(流式展示)。**注:`Default`/`serde(default)` 只救 `..Default` 站点与反序列化;全仓大量穷举 struct 字面量(`ModelRequest` ~15、`ModelResponse` 33、`Message::Assistant` 21 构造+4 解构)须逐处补字段,清单见 tasks 1.1/1.2。**
3. **per-model 能力表(`model_meta.rs`,provider 分表)**:`anthropic_thinking_capability(model)` → `None | Budget{effort} | Adaptive{can_disable,max_effort}`;`openai_thinking_capability(model)` → `None | Effort{max}`。仿 `WINDOW_TABLE` 静态子串匹配、最具体在前;**未知模型保守回退**(Anthropic 未知→Adaptive 现代默认;OpenAI 未知→None 不发 effort),降级**不报错**。
4. **Anthropic 链(anthropic_wire + anthropic_stream)**:请求侧按能力映射 `thinking`/`output_config.effort`/`display:"summarized"`,Off 分模型处理(可关→`type:"disabled"`;恒开的 Fable5/Mythos5→最低 effort + TUI 提示;老模型→省略);Assistant content 把上轮 `ThinkingBlock`(含 signature、含空文本 omitted 块)**原样排首位**回传。流式解析 `thinking_delta`(→on_thinking)+ `signature_delta` + `redacted_thinking`,`finish` 装进 `ModelResponse.thinking`。
5. **OpenAI 链(wire + stream)**:reasoning 模型**恒**用 `max_completion_tokens`(非 `max_tokens`,模型属性、与是否开思考无关,否则 400);开思考时**额外**加顶层 `reasoning_effort`(Depth 同名映射);回传侧忽略 signature(OpenAI 不回传推理正文);stream 可选解析兼容网关的 `delta.reasoning_content`→on_thinking(官方无则展示留空)。
6. **agent-loop 接线**:`Agent` 加 `set_thinking_depth(Arc<Mutex<Depth>>)`(仿 `set_permission_mode`,注入点在 `tui/mod.rs` 装配后),**主循环 + 触顶 forced-final 两处** ModelRequest 均读快照填 `thinking`、两处 `Message::Assistant` push 均带 `response.thinking`;开思考时 `tool_choice` 保持 auto(不加强制工具,否则 Anthropic 400);**`/model` 切换剥离 history 内 `Message::Assistant.thinking`**(避免跨模型 signature 回传 400)。
7. **config**:`thinking: Depth` 默认档(照 `keep_recent_turns` 先例连改 RawConfig/merge/resolve/DEFAULT 四处),**默认 `low`**(用户已定)。
8. **`/think` 命令**:`/think`(裸=查询当前档+列可选)、`/think off|low|medium|high|xhigh`(即时设档,存活当前会话、不回写配置);footer 显示当前档(仿权限模式指示器)。
9. **TUI 展示**:`TranscriptBlock::Thinking(String)` + `AgentEvent::ThinkingDelta`,折叠**复用 Ctrl+O**(折叠出 `✻ Thinking…` 摘要行,展开出灰字思考正文)。

## Impact

- 修改 capability:
  - `provider-abstraction`:**MODIFY** —— `Depth`/`ThinkingConfig`/`ThinkingBlock` 三类型;`ModelRequest.thinking`、`ModelResponse.thinking`、`Message` 思考载体、`DeltaSink::on_thinking`;能力表三态语义 + 降级契约。
  - `anthropic-transport`:**MODIFY** —— 请求侧 adaptive+effort/enabled+budget/disabled/display 映射;流式 thinking/signature/redacted 解析;多轮原样回传约束;tool_choice auto/none 限制。
  - `openai-transport`:**MODIFY** —— `reasoning_effort` 映射 + reasoning 模型 `max_completion_tokens`;可选 `reasoning_content` 解析。
  - `config-layering`:**MODIFY** —— `thinking` 默认档字段(默认 low)。
  - `builtin-commands`:**MODIFY** —— `/think [off|low|medium|high|xhigh]`。
  - `agent-loop`:**MODIFY** —— 每轮 depth 源 + 回传 thinking 入 history + 开思考不强制 tool_choice。
  - `tui-shell`:**MODIFY** —— `TranscriptBlock::Thinking` + `ThinkingDelta` 事件 + Ctrl+O 折叠 + footer 档位指示。
- Affected code:`src/provider/mod.rs`、`src/provider/model_meta.rs`、`src/provider/anthropic_wire.rs`、`src/provider/anthropic_stream.rs`、`src/provider/wire.rs`、`src/provider/stream.rs`、`src/provider/openai.rs`、`src/provider/anthropic.rs`、`src/provider/mock.rs`、`src/agent/message.rs`、`src/agent/mod.rs`、`src/agent/compacting.rs`、`src/agent/context.rs`、`src/config/mod.rs`、`src/app.rs`、`src/session/mod.rs`、`src/tui/mod.rs`、`src/tui/command.rs`、`src/tui/channel.rs`、`src/tui/app.rs`、`src/tui/render.rs`(后半为加字段波及的穷举构造/解构点,见 tasks 1.1/1.2)。
- **无新依赖**(thinking 结构用现成 `serde`)。
- **风险**:Anthropic 多轮原样回传契约(漏/改 signature→400)、构造点扩散(用 Option/Vec+Default 压最小)、能力表随模型发布腐化(照 WINDOW_TABLE 保守回退 + 未来接 `/v1/models` capabilities)、切模型时旧思考块需剥离、assistant 快照 churn。见 design。
- **Non-Goals(v1)**:`max` 档;Anthropic `/v1/models` capabilities 端点动态检测(v1 用静态表);OpenAI Responses API 的 reasoning summary / `encrypted_content` 跨轮无状态保留;Gemini/DeepSeek provider 映射;思考 token 计数展示(`thinking_tokens`);interleaved thinking 精细控制;Shift+Tab 或独立键位循环档位。(注:`/model` 切换剥离旧思考块 **已纳入 v1**,见 What Changes §6 / design D11。)
- **边界**:属核心 provider 层纵向加法(非 TUI 加法线程);与既有 diff/markdown 渲染线程正交。
- 回退:`Depth::Off` 时全链不发任何思考字段、`Message` 思考载体为空——等价于引入前行为。

## 已定(审查后,用户拍板)
- **Depth 档位集** = `off/low/medium/high/xhigh`(含 xhigh,贴 Claude effort 菜单;不支持 xhigh 的模型经 `max_effort` 封顶降级到 high)。
- **`/think` 裸命令** = 查询当前档 + 列可选(无副作用);非法参数 `/think foo` 走 `ThinkArg::Invalid` 出 Notice、不改档。
- **切模型旧 thinking 块** = v1 **做剥离**(`/model` 切换清空 `Message::Assistant.thinking`),规避同 provider 换型跨模型 signature 回传 400。
- **会话持久化 signature** = 接受(加密串、非凭据、回传所需);`Message::Assistant.thinking` 字段级 `#[serde(default)]` 保旧文件向后兼容。
