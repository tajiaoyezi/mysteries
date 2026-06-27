# 2026-06-27 · 11 · archive add-tui-theme

## 决策

- **TUI cut2b-a:全主题 + 带色快照基建**(颜色首次真上)| 主导:cut2 拆分的 cut2b 前半 | 依据:§8 / 设计规范 01
- **D1 `Theme` = 扁平命名色场 + `midnight()`/`daylight()`** | 弃宏/map;扁平字段直观、token単測逐项断言
- **D2 token単測锁值** —— 这是「带色 insta 用 token 名」成立的前提:token 名映射不抓值漂移,值漂移由本単測兜底
- **D3 `render(frame, state, &Theme)` 替硬编码 ANSI 色**(3 调用点,run_tui 默认 Midnight,render 仍纯函数)
- **D4 `buffer_to_styled` 反查 token 名**(确定性首名)→ 带色快照;**两层锁色**:值漂移→単測、赋错→styled insta | 同值 token 取首名(歧义,文档标注;Daylight `text.title==text.primary` 即实例,色正确)| 弃原始 Rgb hex(噪声大、脱钩语义,用户否决)
- **D5 既有 6 快照迁移 styled**,`buffer_to_text` 退役(无双表示)
- **D6 Daylight 测试传入、不引 config 字段 → 不 MODIFY `config-layering`**;运行时切换(config `theme` / `/theme`)延后 | 只 MODIFIED `tui-shell`
- **D7 测试分界**:token 值=単測;themed render/styled=带色 insta;welcome themed=对眼
- **对眼停点 §6.1**:welcome 两主题(连配色)
- **审查修正**:
  - ① §6.1 对眼**打回**:副标「AGENT…终端编码助手」误用 `text.secondary` → 应 `accent.primary`(原型两套都紫);顶栏品牌明暗反了(`mysteries`=text.muted 最暗、`agent`=text.secondary 更亮)→ 改 `mysteries`=text.secondary+bold 亮于 `agent`=text.muted | 主 agent 对眼(对 原型 midnight-01/daylight-01)
  - ② 最终 review 抽查迁移快照:permission 动作行 `[n · 拒绝]`=`warning.fg`,但 01 明列「拒绝=error.fg」→ 应 `error.fg` | 主 agent 抽查;**归 cut2b-b**(permission 帧对眼在 b,b 加 diff body 重渲时顺修)
- **里程碑**:TUI 呈 01 语义双调色板;配色漂移由 token単測 + styled insta 双层拦截

## 变更

- 新增 `src/tui/theme.rs`(`Theme` 19 token + `midnight()`/`daylight()` + token単測 + `token_name` 反查);`render.rs`(`render(&Theme)` 语义上色 + `buffer_to_styled`,退 `buffer_to_text`);`mod.rs`(run_tui 默认 Midnight);6 快照迁移 styled + 1 Daylight welcome 新快照
- 验证:`theme.rs` 19×2 token 逐值对 01;`cargo test` 122 passed / 1 ignored;`clippy` 零警告;`fmt` 通过;**零新依赖**(`Cargo` 无 diff)
- archive:`changes/add-tui-theme` → `changes/archive/2026-06-27-add-tui-theme`;`specs/tui-shell` +3 ADDED

## 待决

- **cut2b-b `add-tui-components`**:C6 diff body(args 派生:write `content`→+;edit `old_string`→−/`new_string`→+)+ C7 致命框 + transcript 滚动 + spinner 动画 + insta 全锁 + permission/fatal×2 themed 对眼;**顺修 permission 动作行 `[n · 拒绝]`→`error.fg`**(01「拒绝=error.fg」,cut2b-a 抽查发现)
- 256/16 色降级 → 路线图 1.4;运行时主题切换(config `theme` 字段 / `/theme`)延后
- `ToolOutcome.exit` 字段(恢复工具卡 exit foot)→ tool-system / 收尾
- 内置命令(C8/C9)、Anthropic、step5 其余收尾

## 引用

- change:`add-tui-theme`(rationale / rejected alternatives 全量见 design.md D1–D7;archive 路径 `changes/archive/2026-06-27-add-tui-theme`)
- 技术方案 §8 / §12 step4
- `设计规范/01-设计令牌`(19 token 两调色板)、`03` C5/C6、`原型截图/midnight-01·daylight-01`(welcome 对眼基准)
- 前置 change:`add-tui-events`(决策记录 10)
- session log:无专属 checkpoint —— 子 agent propose + implement(停点 §6.1 welcome 对眼);主 agent review(核 theme.rs 逐值对 01、§6.1 对眼两轮打回[副标 accent.primary + 顶栏明暗序]、最终抽查发现 permission `[n·拒绝]` 配色归 cut2b-b)+ commit / archive
