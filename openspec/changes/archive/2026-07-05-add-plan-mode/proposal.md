# add-plan-mode(L1 foundation,第 1/2 步)

## Why

agent 现在**没有 plan / research-first 模式**:拿到任务直接开干、边想边改。想要一个**强 plan 模式**(对标 Droid Missions 的精简单 agent 版):先**只读调研**、产出一份**结构化 plan**(带步骤 + 每步验收判据),你**批准**后再执行。这也是 CLAUDE.md「先想后做、定义可验证成功判据」在产品里的落地。

L1 拆两步(用户已定):
- **本 change(foundation)= 机制**:`PermissionMode::Plan` + schema-omit(plan 期只给只读工具)+ `submit_plan` 结构化工具 + 批准即执行。产出一个**能跑的**结构化 plan 模式。
- **下一 change(enrichment)**:validation contract **强制**(逐步验判据)+ 进度跟踪 + 持久化。

## What Changes

1. **`PermissionMode::Plan`**(第 4 模式,Shift+Tab 可切入):plan 期 agent 只读调研、不改不执行。
2. **schema-omit**:Plan 模式下发给模型的工具 schema **只含只读工具 + `submit_plan`**(Edit/Execute 类摘掉、模型压根看不见);非 Plan 模式反过来摘掉 `submit_plan`。
3. **`submit_plan` 工具**(结构化):schema = `{ title, steps: [{ description, validation }] }`;经 `PlanApprover` seam 呈递审批 → **批准** = mode 翻 `Plan → AcceptEdits`、agent 续跑执行;**驳回** = 理由回给模型改、留 Plan。
4. **`ask_user` 工具**(agent 主动澄清):`ask_user(question, options:[{label, description}], allow_multi?, allow_other?)` → 经 `UserPrompter` seam 弹 A/B/C + 补充框、**阻塞等你选** → 选项 + 补充文本回给模型。权限级 `ReadOnly`(**非 plan_only**)→ 任何模式可用、**Plan 期研究尤其用它澄清岔路**,不占 Shift+Tab 模式位。
5. **共用交互 seam**:`submit_plan` 审批与 `ask_user` 提问本质都是「工具阻塞、等你结构化输入」,复用同一根 TUI 交互通道(仿现有 `PermissionDecider`/`channel`);两个 focused trait(`PlanApprover`/`UserPrompter`)共享底层 channel 管线,不各造一套。
6. **plan 模式系统指令**:mode==Plan 注入 —— **用户只是问 → 直接答;撞到岔路/歧义 → `ask_user` 弹选项让用户定;用户要执行任务 → `submit_plan` 交带每步 validation 的结构化 plan**;别改/执行。
7. **TUI**:Plan 指示 + 两个交互框(plan 审批、A/B/C 提问)+ 批准时 mode flip。`TestBackend`+`insta` 事后快照,不走 red-green。

## Impact

- 修改 capability:
  - `permission-gate`:**MODIFY** —— 加 `Plan` 变体、`auto_allows(Plan, *)`、cycle 纳入 Plan、label。
  - `tool-system`:**MODIFY** —— `Tool::plan_only()`(默认 false)+ 按 mode 装配 schema(schema-omit)。
  - `agent-loop`:**MODIFY** —— 每轮读 mode → mode-aware schemas;Plan 时注入 plan 系统指令;`submit_plan` 批准/驳回不打断 loop。
  - `builtin-tools`:**ADD** —— `submit_plan`(+ `PlanApprover`)与 `ask_user`(+ `UserPrompter`)两工具,共用交互 channel;real=TUI、test=mock。
  - `tui-shell`:**MODIFY** —— Plan 指示 + Shift+Tab 达 Plan + 共用交互 channel 渲染 plan 审批框与 A/B/C 提问框(shell,insta 事后)。
- Affected code:`src/permission/mod.rs`、`src/tool/mod.rs`(`Tool::plan_only` + `ToolRegistry::schemas_for`)、`src/agent/mod.rs`(mode 源 + mode-aware schemas + plan 指令)、`src/tool/plan.rs`(新:`submit_plan`/`PlanApprover`/`Plan`)、`src/tool/ask.rs`(新:`ask_user`/`UserPrompter`/`Question`/`Answer`)、`src/app.rs`(注册两工具)、`src/tui/*`(指示 + 两交互框 + mode flip)。
- **无新依赖**(plan / question 结构用现成 `serde`)。
- **不在本 change**(→ enrichment):validation **强制**执行、进度跟踪、持久化。
- **不在本 change**(→ 单独):web 工具 `ReadOnly`→`Network` 权限级(安全复审 finding 3)。原因:Plan 模式 research 需要 web 工具**可用**(只读研究),`Network` 级与 schema-omit 的交互需专门设计,不塞进 foundation;foundation 里 web 工具保持 `ReadOnly`、Plan 期照常可研究。
- 回退:纯增一个模式 + 一个工具;不切入 Plan 即完全无影响。

## 待定(请你审设计时拍板,见 design.md)
- Plan 在 Shift+Tab cycle 的位置(默认:追加到末尾 `Normal→AcceptEdits→Yolo→Plan→Normal`)。
- 批准后切到哪个模式(默认 `AcceptEdits`)。
- foundation 是否需再拆(我判断不必:submit_plan 是 L1 的灵魂,拆了就退化成弱 plan)。
