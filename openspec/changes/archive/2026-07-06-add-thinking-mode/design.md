## Context

现状(已实读):`ModelRequest`(provider/mod.rs:34-40)= `model/messages/tools/max_tokens`,**仅 `derive(Debug,PartialEq)`、无 `Default`**;`ModelResponse`(:54-60,**已 `derive Default`**);`Message::Assistant { text, tool_calls }`(agent/message.rs:4-11,`Message` 无容器级 serde default)既进 history 又被两 wire 序列化。`anthropic_wire::serialize_request`(fn 定义 :7、body 组装 :56)只拼 `model/max_tokens/messages/system/tools`;`wire.rs`(OpenAI,:6)只拼 `max_tokens`。两 stream 无 thinking 分支。`model_meta.rs` 已有 `WINDOW_TABLE`(pattern→context window)静态子串匹配先例。`config`(config/mod.rs)有 `keep_recent_turns` 等字段 RawConfig/merge/resolve/DEFAULT 四处一致改的完整先例。`command.rs` 有 `Command`/`BuiltinCommand`/`COMMANDS:[_;7]`/`parse_command`(返 `Option<Command>`)闭环 + `command_metadata_covers_all_builtin_commands_and_matches_parser`(:150,实名带后缀)硬编码测试。TUI `TranscriptBlock`(app.rs:48)+`transcript_lines`(render.rs)+`tools_expanded`/Ctrl+O 折叠机制现成。`Agent` 有 `set_permission_mode(Arc<Mutex<PermissionMode>>)` setter+每轮读快照先例;**注入点在 `tui/mod.rs run_tui`(建 Arc:109→装配后 `set_*`:128→存 AppState:164),非 `assemble_agent` 内**。

**Web 已核实(2026-07,官方 docs 逐字确认)** 的跨 provider 事实,是本设计的事实地基:

- **Anthropic 当代**(Opus 4.8/4.7、Sonnet 5、Fable 5、Mythos 5):`thinking:{type:"adaptive"}`,effort 在**独立顶层** `output_config:{effort:"low|medium|high|xhigh|max"}`(`high`=默认=省略;`xhigh` 仅 Fable5/Opus4.8/4.7/Sonnet5)。**必须 `thinking.display:"summarized"` 才拿到思考文字**(当代默认 `"omitted"`=空 text+仅 signature);设 summarized 后 `thinking_delta` 正常流式。`type:"enabled"+budget_tokens` 在 Opus 4.8/4.7/Sonnet 5 上 **400**。
- **Anthropic 老模型**(Opus 4.5:effort 也支持;Haiku 4.5:仅 budget):`thinking:{type:"enabled",budget_tokens:N,display:"summarized"}`,`budget_tokens<max_tokens` 且 ≥1024。
- **Off 分模型**:Opus 4.8/4.7 不传 `type:adaptive` 即关(也可 `type:"disabled"`);Sonnet 5 默认开、须显式 `type:"disabled"`;**Fable 5/Mythos 5 恒开、`type:"disabled"` 报错关不掉**。
- **多轮 tool_use**:上轮 `thinking`/`redacted_thinking` 块(含 signature、含空文本 omitted 块)必须**原样、排 content 首位**回传,改动→400(`'thinking' ... cannot be modified`);读它做展示不算改;切模型须剥离旧思考块。
- **其它 Anthropic 约束**:当代模型拒非默认 `temperature/top_p/top_k`(我们不发,安全);`tool_choice` 强制工具(any/tool)与思考不兼容(我们不发 tool_choice=默认 auto,安全);流式 `signature_delta` 在 `content_block_stop` 前。
- **OpenAI**:Chat Completions 顶层 `reasoning_effort:"low|medium|high|..."`;reasoning 模型用 `max_completion_tokens`(非 `max_tokens`);官方**不回传**推理正文(仅 `usage...reasoning_tokens`),兼容网关(vLLM/DeepSeek)有 `delta.reasoning_content`(与 content 同级)。reasoning 模型拒 temperature(安全)。

## Goals / Non-Goals

**Goals:** 统一 `Depth` 抽象 + per-model 能力表 + Anthropic/OpenAI 双链请求映射 + Anthropic 思考流式解析与原样回传 + `/think` 命令 + config 默认档 + TUI 折叠展示。降级不报错。

**Non-Goals(v1):** `max` 档、`/v1/models` 动态能力检测、OpenAI Responses API summary/encrypted_content、Gemini/DeepSeek、thinking_tokens 计数展示、interleaved 精细控制、键位循环档位。(`/model` 剥离旧思考块见 D11,已在 v1。)

## Decisions

- **D1 统一 `Depth { Off, Low, Medium, High, Xhigh }`**(provider 无关,`serde` rename_all lowercase)。唯一进 `config`/`/think`/`ModelRequest.thinking` 的表示。`Xhigh` 纳入以贴 Claude effort 菜单;不支持 xhigh 的模型在映射时**降级到 high**(能力表 `max_effort` 封顶)。`max` 留后续。

- **D2 类型(集中定义于 provider/mod.rs)**:
  - `ThinkingConfig { depth: Depth }`(v1 只带 depth;留结构以便后续加 display/budget 覆盖)。
  - `ThinkingBlock { text: String, signature: Option<String>, redacted: bool }`,`Serialize/Deserialize/Clone/Debug/PartialEq/Default`。`text` 供展示(可空)、`signature` 供 Anthropic 原样回传(OpenAI 恒 None)、`redacted` 标记 `redacted_thinking`。
  - `ModelRequest.thinking: Option<ThinkingConfig>`(**并给 `ModelRequest` 加 `#[derive(Default)]`**,现无);`ModelResponse.thinking: Vec<ThinkingBlock>`(已 Default);`Message::Assistant.thinking: Vec<ThinkingBlock>`(**用户已定 Vec**,容 redacted+多块;**字段级 `#[serde(default)]`** 保旧 session 向后兼容)。`DeltaSink::on_thinking(&self, _t: &str) {}` 默认空实现。
  - **机械爆炸面(实读,勿低估)**:`derive(Default)`/`#[serde(default)]` 只救用 `..Default::default()` 的少数站点与 serde 反序列化;而全仓大量穷举 struct 字面量(无 `..rest`)须**逐处补字段**——`ModelRequest` ~15 处、`ModelResponse` 33 处、`Message::Assistant` 21 构造 + 4 解构。清单见 tasks 1.1/1.2。

- **D3 能力表 provider 分表(model_meta.rs,静态子串匹配、最具体在前,仿 WINDOW_TABLE)**:
  - `AnthropicThinking { None, Budget { effort: bool }, Adaptive { can_disable: bool, max_effort: Depth } }`。
    条目示例(`max_effort` 只能取 Depth 变体 `Off/Low/Medium/High/Xhigh`,**不得写不存在的 `Max`**):`claude-opus-4-8/4-7`、`claude-sonnet-5` → `Adaptive{can_disable:true, max_effort:Xhigh}`(均支持 xhigh);`claude-fable-5`/`claude-mythos-5` → `Adaptive{can_disable:false, max_effort:Xhigh}`;`claude-opus-4-6`/`claude-sonnet-4-6` → `Adaptive{can_disable:true, max_effort:High}`(无 xhigh,封 high;v1 无 Max 档);`claude-opus-4-5` → `Budget{effort:true}`;`claude-haiku-4-5` → `Budget{effort:false}`。**未知 claude-\* → `Adaptive{can_disable:true, max_effort:High}`**(现代默认;adaptive+summarized+effort 在任何 adaptive 模型上安全,只有早于 adaptive 的老模型会 400,而那些都在表里)。
  - `OpenAiThinking { None, Effort { max: Depth } }`。条目:`o1/o3/o4`、`gpt-5*` → `Effort{max:High}`(v1);**未知 → None**(不发 reasoning_effort,防非 reasoning 模型/未知网关 400)。
  - **降级契约(核心)**:能力为 `None` → 忽略 `Depth`、全链不发思考字段、不报错;`Depth::Xhigh` 遇 `max_effort<Xhigh` → 发 high;always-on(`can_disable:false`)遇 `Depth::Off` → 发 `output_config.effort:"low"`(不发 disabled)且 TUI 提示"该模型思考无法关闭"。

- **D4 Anthropic wire 映射(anthropic_wire.rs)**。集中到 `fn anthropic_thinking_body(cap, depth, max_tokens) -> (Option<Value> thinking, Option<Value> output_config)`:
  - `Adaptive` + depth≠Off:`thinking={type:"adaptive", display:"summarized"}` + `output_config={effort: depth.as_effort(capped)}`。depth=Off + can_disable:`thinking={type:"disabled"}`;Off + !can_disable:`output_config={effort:"low"}`(无 thinking)。
  - `Budget` + depth≠Off:`thinking={type:"enabled", budget_tokens:N, display:"summarized"}`,`N=clamp(max_tokens×ratio, 1024, max_tokens-1)`(low .2/medium .5/high .8/xhigh .9);`effort:true` 的还可并发 `output_config.effort`(v1 可省,仅 budget)。depth=Off:省略 thinking。**budget 守卫**:`max_tokens` 为 `None` 或 `<1025` 时**不发 budget_tokens**(退回省略 thinking)——否则 `clamp(_,1024,max_tokens-1)` 在 `min>max` 时 u32 panic(默认路径 `max_tokens.unwrap_or(1024)` 恰触边界)。
  - `None`:不动 body。
  - **Assistant 回传**:`serialize_request` 的 Assistant 分支(:20)把 `msg.thinking` 的每块作为 content 数组**首批**元素:`{type:"thinking", thinking:text, signature}` 或 `{type:"redacted_thinking", data:signature}`,排在 text/tool_use 之前、逐字节原样。空 `thinking` 数组则维持现状。

- **D5 Anthropic stream 解析(anthropic_stream.rs)**:`content_block_start` 识别 `type=="thinking"|"redacted_thinking"` 建块;`content_block_delta` 加 `thinking_delta`(累积 text + 调 `on_thinking` 流式)与 `signature_delta`(累积 signature);`finish` 把累积块推入 `ModelResponse.thinking`(保序、含空 text 块)。

- **D6 OpenAI wire/stream(wire.rs/stream.rs)**:`serialize_request` 依 `openai_thinking_capability`——**`max_completion_tokens` 与 `reasoning_effort` 解耦**(reasoning 模型用 `max_completion_tokens` 是模型属性,与是否开思考无关):`Effort`(reasoning 模型)→ 输出上限字段**恒**用 `max_completion_tokens`(不论 depth,含 Off,否则见 `max_tokens` 400);`Effort` + depth≠Off → **额外**发顶层 `reasoning_effort=depth.as_effort(capped)`;`None` → 不动 body(保持 `max_tokens`)。**注**:若沿用旧门控"depth=Off 保持 max_tokens",`/think off` 会让 reasoning 模型 400(审查 F13)。Assistant 分支不发 signature。`stream.rs` 可选:`delta.reasoning_content`→`on_thinking`(官方无,兼容网关有;无则 TUI 思考区留空)。

- **D7 agent-loop(agent/mod.rs)**:`Agent` 加 `set_thinking_depth(Arc<Mutex<Depth>>)`(字段默认 `Low`,**不改 `Agent::new` 签名**,仿 `set_permission_mode`)。`run_observed` **两处** ModelRequest 均填 `thinking=Some(ThinkingConfig{depth})`:主循环 :150(loop 顶读快照)与 **forced-final :266**(在 for 外,**须重新读一次 depth 快照**,不能复用循环内已出作用域局部变量,否则触顶最终轮静默不思考);**两处** `Message::Assistant` push 均带 `response.thinking`(:166、:278);`run_single_turn`(:27)仅补字段置 `None`。depth=Off 也传,交 wire 判 None/disabled;**不设 tool_choice**(默认 auto)。注入点在 **`tui/mod.rs run_tui`**(建 Arc→装配后 `assembled.agent.set_thinking_depth`→存 AppState,仿 `set_permission_mode`),`assemble_agent` 签名不动。

- **D11 切模型剥离(agent/mod.rs `set_model`/`SetModel` 路径)**:`/model` 切换时清空 caller `history` 内所有 `Message::Assistant.thinking`。因 thinking 块常驻 history 且每轮原样回传含 signature,同为 Anthropic 的换型(如 opus-4-8→sonnet-5)带 tool_use 回传跨模型 signature 会 400(审查 F14);剥离从根上规避。OpenAI 侧本不发 signature,清空亦无害。

- **D8 config(config/mod.rs)**:`Config.thinking: Depth` + `RawConfig.thinking: Option<Depth>`(`#[serde(default)]`),`merge` 加 `project.thinking.or(user.thinking)`,`resolve` 填 `DEFAULT_THINKING=Depth::Low`。**不做能力校验**(model 可 /model 运行时切换,校验/降级在 wire 层按能力表做)。

- **D9 命令(command.rs)四处同改**:`Command::Think(ThinkArg)`(`ThinkArg = Query | Set(Depth) | Invalid(String)`——`parse_command` 返 `Option<Command>` 无错误通道、`Depth` 闭合枚举容不下非法串,故 `/think foo` 落 `Invalid("foo")`,审查 F11)、`BuiltinCommand::Think`、`COMMANDS` 扩到 8(同步 `command_metadata_covers_all_builtin_commands_and_matches_parser` 的 `expected` 名字数组与 `[_;7]→[_;8]`)、`parse_command` 解析 `/think`(裸=Query)/`/think <depth>`(Set)/`/think <非法>`(Invalid)。落地:app.rs 命令分发改共享 `Arc<Mutex<Depth>>`,`Invalid` 出 Notice 列合法取值、不改档;footer 显示当前档(仿权限模式指示器)。纯逻辑,强制 TDD。

- **D10 TUI 展示(channel/app/render)**:`AgentEvent::ThinkingDelta(String)`;`ChannelSink::on_thinking` 发之。`TranscriptBlock::Thinking(String)`,`apply()` 加分支仿 `TextDelta` 累积(追加 last_mut 或新建)。`render.rs transcript_lines` 加 `Thinking` 分支:折叠(复用 `tools_expanded`+Ctrl+O)时出一行 `✻ Thinking…(Ctrl+O 展开)` 摘要,展开时渲灰字(`text_muted`)思考正文。always-on 模型 Off 的"关不掉"提示走一行 notice。insta 事后快照(暗/亮双主题),不 red-green。

## Alternatives considered

- **按 provider 一刀切(Anthropic=budget / OpenAI=effort)**——被 web 事实证伪:当代 Claude 拒 budget、走 adaptive+effort。弃,改 provider 分表三态能力。
- **effort 写进 `thinking.effort` 或裸顶层 `effort`**——官方确认是**独立顶层 `output_config.effort`**。弃错写法。
- **未知 Anthropic 模型回退 budget_tokens**——当代模型 400。弃,未知回退 adaptive。
- **不设 `display:"summarized"`**——当代默认 omitted、思考文字恒空、TUI 展示形同虚设。弃,思考开启即发 summarized。
- **`Message::Assistant.thinking` 用 `Option`**——容不下 redacted+多块;用户已定 `Vec`。
- **能力校验放 config.resolve**——与 /model 运行时切换解耦冲突。弃,放 wire 层按表降级。

## Risks / Trade-offs

- **原样回传契约**:漏/改 signature 或未排首位→400。测试须覆盖"带 tool_use 多轮回传 thinking 块字节一致"。
- **能力表腐化**:新模型未入表→按 provider 默认(Anthropic adaptive / OpenAI None)保守处理;记 D3,后续接 `/v1/models` capabilities 权威源。
- **切模型旧思考块**:v1 **做剥离**(D11)——`/model` 切换清空 `Message::Assistant.thinking`。因同 provider 换型(Anthropic→Anthropic)带 tool_use 回传跨模型 signature 会 400(非"静默忽略、仅费 token";那只对切到 OpenAI 侧成立),故必须剥离,与 Context 所述硬约束一致。
- **构造点扩散 / 快照 churn**:Option/Vec+Default 压编译面;TranscriptBlock 加变体命中 render 快照,事后 review 接受(注意勿误接 git status 里既有 `.snap.new`)。
- **OpenAI 官方展示留空**:Chat Completions 不回推理正文,TUI 思考区在官方 OpenAI 上为空(仅 Anthropic/兼容网关有内容);属预期,footer 档位仍显示。
