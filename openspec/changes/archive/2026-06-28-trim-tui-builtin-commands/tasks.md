# Tasks — trim-tui-builtin-commands

> **流程倒置补救(诚实标注)**:本 change 的 **code 已先行实现并验证**(主 agent 误把删 `/login` `/logout` + `/model` 无参补引导当小修直接做了),仅 `builtin-commands` / `tui-shell` spec 漂移未同步。故本 change **无新增代码、无新红灯**:任务 = ① 补 spec 使其与既有实现一致(本轮)② 复述已通过的既有验证。权威次序 code > spec,spec 以已测 code 为准回填。
> 边界:本轮**只写 change 物料,不碰任何 `src`**(src 已是最终态,改动反而再漂移);不动主 specs(archive 时才 sync);不 commit / archive。

## 1. spec 同步(本轮 propose)

- [x] 1.1 `specs/builtin-commands/spec.md` MODIFY「slash 命令解析」:`Command` 枚举去 `Login` / `Logout`;明确 `/login` `/logout` → `Unknown`、不入元数据清单。
- [x] 1.2 `specs/builtin-commands/spec.md` MODIFY「命令执行语义」:去 Login/Logout 占位语义;C8 帮助块 7 → 6;占位 scenario 改为「未知命令(含已删的 /login /logout)」。
- [x] 1.3 `specs/builtin-commands/spec.md` MODIFY「/model 查看与运行时切换」:无参 notice 含切换引导 `当前 model: {model} — 输入 /model <name> 切换`;scenario 增「输出含切换引导 `/model <name>`」。
- [x] 1.4 `specs/tui-shell/spec.md` MODIFY「命令块渲染(C8 / C9 / notice)」:C8 帮助块 7 → 6(去 `/login` `/logout`);标注 C9 `tools: 7` 指内置工具数、不变。
- [x] 1.5 `proposal.md` 记 why / what / impact + 流程倒置说明;`openspec validate trim-tui-builtin-commands --strict` 通过。

## 2. 既有实现核对(已先行完成,复述)

> 以下均为 **code 既成事实**,本轮仅核对 spec 与之一致,不改 src。

- [x] 2.1 `src/tui/command.rs`:`Command` 枚举 = `Help` / `Clear` / `Model(Option<String>)` / `Status` / `Exit` / `Compact` / `Unknown(String)`(无 `Login` / `Logout`);`COMMANDS` 元数据 6 项;`/login` / `/logout` 解析为 `Unknown`。
- [x] 2.2 `src/tui/app.rs`:无 `Login` / `Logout` 执行臂;`Command::Model(None)` notice = `当前 model: {model} — 输入 /model <name> 切换`;`Unknown(name)` notice = `未知命令: /{name}`。
- [x] 2.3 `src/tui/render.rs` + 快照:C8 帮助块 6 命令(`tui_help_block` 已去 `/login /logout 凭据占位` 行);`tui_command_completion` 候选回归 6 命令。

## 3. 既有验证复述(无新红灯,均已通过)

- [x] 3.1 `cargo test --lib` 全绿(**264** 测试),含 `parse_command` 识别 `/login` `/logout` → `Unknown`、`command_metadata` 6 项、`/model` 无参 notice 含 model 与「model」字样、两快照锁定一致。
- [x] 3.2 `cargo clippy` 无警告;`cargo fmt --check` 通过。
- [x] 3.3 已 review(主 agent)。无新增依赖。
