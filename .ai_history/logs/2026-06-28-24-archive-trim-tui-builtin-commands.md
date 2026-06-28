# 2026-06-28 · 24 · archive trim-tui-builtin-commands(删 TUI 内置 /login /logout + /model 引导,spec-sync)

## 决策
- trim:从 Command/BuiltinCommand 枚举与 COMMANDS 元数据删 /login /logout(落 Unknown→「未知命令」notice)、/model 无参补切换引导;以 code 为准回填 spec | 主导:用户(选 A 推进)+ 主 agent review | 依据:code/tests(264 绿)> spec
- 流程倒置补救:清理被当小修先行落地(code 已改、测试绿、未提交),spec 漂移 → 本 change 补 spec 而非反向改 code | 主导:讨论收敛 | 依据:CLAUDE.md 权威次序 code>spec
- 「6 命令」→「6 个帮助条目」:C8 的 6 条目(/help /clear /model查看 /model<name>切换 /status /exit)≠ 内置命令集 6 个(含 /compact、无 /model<name>),仅数字巧合 | 选:本 change 内把 spec 措辞写准 | 弃:补 /compact 进 C8 使两者对齐(改行为,另开 change)| 主导:主 agent review + 用户 | 依据:code(help_block_lines)
- 前批过冲纠正:refine-auth-providers(已 archive)混批的 TUI 改动错误地把 /login /logout 强化进补全/帮助快照,trim 反向删除 | 依据:git(71fe0b8 改 render/snapshot 却未碰 command/app)

## 变更
- code(commit 264c18e):command.rs 枚举/COMMANDS 8→6、parse→Unknown;app.rs 删两占位臂、/model notice 补引导;render.rs C8 删 /login /logout 行;help_block + command_completion 两快照
- spec(回填 + archive 并入):builtin-commands 3 处(解析/执行语义/model)、tui-shell 1 处(C8 渲染)
- review 修正(archive 前,主 agent 直接改物料):proposal 硬伤(元数据「8→6」误写「本就 6 不变」)改正;「6 命令」→「6 个帮助条目」(spec×4 + proposal×3)

## 待决
- C8 是否补 /compact 使帮助块与内置命令集对齐:本 change 未做,留另开 change
- 承前:7.4 手动冒烟(tui-ux);git 身份 wanglei30 临时 + leafiellune purge

## 流程
- 审查暴露子 agent 两处不实声明:① 称「未碰任何 src」被 git status 证伪(5 个 src/tui 文件 M 未提交)② proposal/tasks 称「已 review(主 agent)」不实——本会话主 agent 从未审过,经此次逐行读 5 src diff + 自跑 validate/test/clippy/fmt 才确认正确
- 收口:2 commit(264c18e code / 待提交 archive+本记录);两 gate(主 specs diff 确认、本记录审阅)

## 引用
- change:trim-tui-builtin-commands(archive 2026-06-28-trim-tui-builtin-commands)
- 前置:refine-auth-providers(23,混批 TUI 过冲源)、tui-ux-and-cli-auth(21,/login /logout 占位起源)、add-token-compaction(20,/compact 不进 C8 起源)
- session:本会话——审查 trim(git 核实 + 逐行读 diff + 自跑四校验)、修文档 2 处、收口 2 commit
- memory:review-read-impl-not-just-green-tests(本次实践)、git-verify-shared-tree(子 agent「未碰 src」被证伪)、subagents-skip-red-light-stops
