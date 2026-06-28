## Why

auth 已迁至 CLI(`mysteries auth login` / `logout` / `list`,见已归档的 `refine-auth-providers`),TUI 内的 `/login` / `/logout` 仅是占位 notice(提示去 config / 环境变量配置),既不再承担实际配置职责、又占帮助块一行,与「凭据全走 CLI」的现状漂移。同时 `/model` 无参仅显示当前 model,未提示「带参可切换」,新用户难发现切换用法。

**流程倒置说明(诚实记录)**:本清理(删 `/login` `/logout` + `/model` 无参补切换引导)已被主 agent 当作小修**先行实现并验证**(264 lib 测试绿、clippy / fmt 过、已 review),但 `builtin-commands` / `tui-shell` spec 未同步 → spec 落后于 code。本 change 形式化该变更,**补 spec 使其与既有实现一致**(本轮不改任何 `src`,src 已是最终态)。权威次序 code > spec,故以已实现、已测的 code 为准回填 spec。

## What Changes

- **删内置 `/login` / `/logout`**:从 `Command` 枚举移除 `Login` / `Logout` 两变体;`/login` / `/logout` 不再是内置命令,经 `parse_command` 落为 `Unknown`(`/login` → `Unknown("login")`),提交后渲染为「未知命令: /login」notice(与其它未知命令同路径)。
- **计数变化**:`Command` 元数据清单 `COMMANDS` 由 **8 → 6**(移除 `/login` `/logout` 两条);C8 帮助块由 **7 → 6** 个帮助条目(去掉 `/login /logout 凭据占位` 一行)。删后内置命令为 `/help` `/clear` `/model` `/status` `/exit` `/compact`(6 个);注:C8 的 6 条目 ≠ 这 6 个命令 —— C8 含 `/model` 查看 + `/model <name>` 切换两行、且不含 `/compact`。
- **`/model` 无参补切换引导**:`Command::Model(None)` 的 notice 由「仅显示当前 model」改为含切换引导 —— `当前 model: {model} — 输入 /model <name> 切换`(无参仅查看、带参切换)。`/model <name>` 切换语义不变。

## Capabilities

### Modified Capabilities

- `builtin-commands`:**MODIFIED**「slash 命令解析」(`Command` 枚举去 `Login` / `Logout`;二者解析为 `Unknown`)、「命令执行语义」(去 Login/Logout 占位语义、帮助块条目 7→6、占位 scenario 改为未知命令)、「/model 查看与运行时切换」(无参 notice 含切换引导 `/model <name>`)。
- `tui-shell`:**MODIFIED**「命令块渲染(C8 / C9 / notice)」(C8 帮助块条目 7→6,去 `/login` `/logout`;C9 `tools: 7` 指 7 内置工具,**不变**)。

## Impact

- **spec**(本 change 唯一改动面):`openspec/specs/builtin-commands/spec.md`、`openspec/specs/tui-shell/spec.md` 的对应 requirement 经 delta MODIFY。
- **code(已先行实现,本轮不动)**:
  - `src/tui/command.rs`:`Command` 枚举去 `Login` / `Logout`;`COMMANDS` 元数据 6 项;`/login` / `/logout` → `Unknown`。
  - `src/tui/app.rs`:删 `Login` / `Logout` 执行臂;`Command::Model(None)` notice 补切换引导。
  - `src/tui/render.rs`:C8 帮助块去 `/login /logout 凭据占位` 行。
  - 快照:`tui_help_block`(去 `/login /logout` 行)、`tui_command_completion`(候选列表回归 6 命令)已更新。
- **验证(已先行通过)**:`cargo test --lib`(264 绿)、`clippy`、`fmt` 均过;已 review。
- **deps**:零变更。
- **不受影响**:agent-loop、provider、CLI `mysteries auth`、其它内置命令(`/help` `/clear` `/model` `/status` `/exit` `/compact`)语义不变。
