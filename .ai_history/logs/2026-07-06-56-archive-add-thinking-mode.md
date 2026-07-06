# 2026-07-06 · 56 · archive add-thinking-mode(思考模式)

## 决策
- 思考控制统一抽象 = `Depth{Off,Low,Medium,High,Xhigh}` + per-model 能力表向下映射(各 wire 层做映射)| 选:统一 Depth + wire 映射 | 弃:按 provider 一刀切(当代 Claude 拒 budget_tokens、走 adaptive+effort)、effort 写 `thinking.effort`/裸顶层(官方是独立顶层 `output_config.effort`)| 主导:用户拍板双 provider,web 逐条核实 | 依据:platform.claude.com / OpenAI docs + code
- Anthropic 当代:`thinking:{type:adaptive,display:summarized}` + 顶层 `output_config.effort`;必须 `display:summarized` 才见思考文字(当代默认 omitted、仅回 signature);Off 分模型(可关→`type:disabled`、恒开 Fable5/Mythos5→`effort low`);带 tool_use 多轮必须原样排首位回传 thinking 块含 signature | 弃:budget_tokens 作主路径(Opus4.8/4.7/Sonnet5 传之 400)| 依据:官方 docs
- OpenAI:reasoning 模型恒用 `max_completion_tokens`(与是否开思考无关)、`reasoning_effort` 仅开思考才发 | 依据:审查 F13 + docs(修「/think off 使 reasoning 模型 400」)
- `Message::Assistant.thinking` = `Vec<ThinkingBlock>` + 字段级 `#[serde(default)]` | 选:Vec(容 redacted+多块)+ serde default(保旧 session /resume)| 弃:Option(容不下)| 主导:用户定 Vec | 依据:审查 F18 用真实 cargo 复现旧文件加载破坏
- 思考展示:默认展开、body 超 12 行只折叠溢出尾部(`… +M 行`)、`✻ 思考` header(text_secondary+BOLD)| 主导:用户真机反馈(默认整块折叠看不到过程、纯灰不好看)| 依据:真机
- `/model` 切换剥离 history 内 `Message::Assistant.thinking`(D11)| 选:做剥离 task | 弃:v1 记为已知限制(同 provider 换型跨模型 signature 回传 400)| 主导:用户拍板 | 依据:审查 F14

## 变更
- provider:`ModelRequest.thinking`/`ModelResponse.thinking`/`ThinkingBlock`/`Depth`/`ThinkingConfig`;`model_meta` 能力表 provider 分表(Anthropic None|Budget|Adaptive、OpenAI None|Effort,未知保守回退);`anthropic_wire`(映射+budget 守卫+首位回传)、`anthropic_stream`(thinking_delta/signature_delta/redacted)、`wire`(effort + max_completion_tokens 解耦)、`stream`(reasoning_content 可选)
- agent-loop `set_thinking_depth` + 主循环&forced-final 双注入 + `/model` 剥离;config `thinking` 默认 Low;command `/think off|low|medium|high|xhigh`(裸=查询、非法=Invalid);TUI `TranscriptBlock::Thinking`+`ThinkingDelta`+折叠+header+footer 档位+恒开 Off 提示
- 顺带(未走单独 change):`PLAN_MODE_INSTRUCTION` + `submit_plan` schema 令每步 description ≤30 字 —— 消除 plan 面板步骤冗长;**受控 spec 漂移**,未同步已归档 plan specs
- 7 spec delta 已 sync 进主 specs;771 tests、clippy 零警告、无新依赖

## 待决
- max 档、`/v1/models` 动态能力检测(v1 用静态表)、OpenAI Responses summary/encrypted_content 跨轮、Gemini/DeepSeek 映射、thinking_tokens 计数展示、interleaved 精细控制、Shift+Tab/键位循环档位
- 折叠快照的 jump-to-bottom pill 属测试视口产物(真机正常终端无)
- plan 简洁是 prompt 软约束,模型听话度待持续真机观察;若正式化需补 plan specs

## 引用
- OpenSpec change:`add-thinking-mode`(archived `2026-07-06-add-thinking-mode`)
- 设计与审查:本 session 两轮 workflow(设计研究 + propose 对抗审查 19 findings,官方 docs 逐条核实 API 事实)
- 跨:plan 顺带修基于 `add-plan-progress`(log 55)
