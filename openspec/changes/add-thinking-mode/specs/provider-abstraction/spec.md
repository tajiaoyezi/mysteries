# provider-abstraction Delta

## ADDED Requirements

### Requirement: 思考深度统一抽象与 per-model 能力表

系统 SHALL 定义 provider 无关的思考深度枚举 `Depth {Off, Low, Medium, High, Xhigh}`(`serde` 小写序列化,默认 `Low`),作为 `config` / `/think` / `ModelRequest` 唯一的思考控制表示;各 provider wire 层 MUST 各自把 `Depth` 向下映射到自身字段,MUST NOT 让 `Depth` 携带任何 provider 专有字段。`Depth::as_effort(cap: Depth) -> &'static str` SHALL 产出 effort 字符串并按 `cap` 封顶降级(如 `Xhigh` 遇 `cap<Xhigh` 返回 `"high"`)。

`ModelRequest` SHALL 增 `thinking: Option<ThinkingConfig>`(`ThinkingConfig{depth: Depth}`)并 SHALL 补 `#[derive(Default)]`(现无);`ModelResponse` SHALL 增 `thinking: Vec<ThinkingBlock>`(已 derive Default);`ThinkingBlock{text: String, signature: Option<String>, redacted: bool}` SHALL derive `Serialize/Deserialize/Clone/Debug/PartialEq/Default`(`text` 供展示可空、`signature` 供原样回传、`redacted` 标记 `redacted_thinking`)。`DeltaSink` SHALL 增 `on_thinking(&self, _text: &str) {}` 默认空实现,既有 impl MUST NOT 因此破坏。新增字段类型 MUST 为 `Option`/`Vec`(缺省即空);**但 `derive(Default)`/`serde(default)` 仅救用 `..Default::default()` 的站点与 serde 反序列化——全仓无 `..rest` 的穷举 struct 字面量 MUST 逐处补字段(以 `..Default::default()` 回填或显式 `thinking: None`/`Vec::new()`,语义不变),不得声称"免改"。

能力检测 SHALL 由 `model_meta` 提供 provider 分表的纯函数(仿 `WINDOW_TABLE` 静态子串匹配、最具体在前):`anthropic_thinking_capability(model) -> {None | Budget{effort} | Adaptive{can_disable, max_effort}}`;`openai_thinking_capability(model) -> {None | Effort{max}}`。**降级 MUST NOT 报错**:能力为 `None` → 忽略 `Depth`、不发任何思考字段;`Depth` 超模型 `max_effort` → 封顶;能力表未命中(未知模型)→ 按 provider 保守默认(Anthropic 未知→`Adaptive{can_disable:true,max_effort:High}`;OpenAI 未知→`None`)。

#### Scenario: Depth as_effort 按 cap 封顶降级

- **WHEN** `Depth::Xhigh.as_effort(Depth::High)`
- **THEN** 返回 `"high"`(封顶);而 `Depth::Medium.as_effort(Depth::Xhigh)` 返回 `"medium"`(不受影响)

#### Scenario: 新增字段不破坏既有构造

- **WHEN** 以 `ModelRequest { model, messages, tools, max_tokens, ..Default::default() }` 构造(不给 thinking)
- **THEN** 编译通过且 `thinking == None`;`ModelResponse` 同理 `thinking` 为空 `Vec`

#### Scenario: 能力表三态与保守未知回退

- **WHEN** 查 `anthropic_thinking_capability("claude-opus-4-8")`、`("claude-opus-4-5")`、`("claude-haiku-4-5")`、`("claude-fable-5")`、`("claude-future-x")`
- **THEN** 依次为 `Adaptive{can_disable:true,max_effort:Xhigh}`、`Budget{effort:true}`、`Budget{effort:false}`、`Adaptive{can_disable:false,..}`、`Adaptive{can_disable:true,max_effort:High}`(未知保守现代默认)

#### Scenario: 不支持思考的模型静默降级

- **WHEN** `openai_thinking_capability` 对未知模型返回 `None`,而请求 `Depth::High`
- **THEN** 该 provider wire MUST 不发任何思考字段、请求照常发出、不报错
