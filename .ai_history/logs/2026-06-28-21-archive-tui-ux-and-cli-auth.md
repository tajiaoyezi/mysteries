# 2026-06-28 · 21 · archive tui-ux-and-cli-auth(1.1.x:TUI 交互 + CLI auth)

## 决策
- 1.1.x 四项一个 change 做完(用户定不拆):① / 命令补全 ② 去鼠标捕获恢复终端原生复制 ③ 状态栏移底 ④ mysteries auth CLI 配置 | 主导:用户(坚持一个 change;主 agent 曾提示点4 是新能力级、建议拆,用户仍要一个)| 依据:技术方案 §13 未规划这 4 项
- 点4 = opencode 式 CLI auth(选 A):凭据/provider 走 CLI 子命令(持久化、key 隐藏输入),TUI /model 仅运行时临时切 | 弃:TUI 内配凭据(回显/安全顾虑)| 主导:用户选 A | 依据:design ④
- 点2 去鼠标捕获:滚轮在 Windows/ConPTY 实测不可用,让位终端原生复制 + scrollback,键盘滚动兜底;spec 用 ADD「终端原生复制」而非 MODIFY「终端生命周期恢复」(核实后者未提 capture)| 主导:主 agent | 依据:code 核实 / design ②
- 点3 状态栏移底 = adapt 设计规范 02(状态行移最底,贴 claude code + D2 字面)| 主导:用户 | 依据:design ③
- 安全红线(点4):key 隐藏输入(crossterm raw 不回显)+ SecretString 不入日志/错误 + Unix 0600 + write_config/credential merge 保留(不整覆盖、不重复行)| 主导:主 agent | 依据:design / spec
- 一个 change 两组并行 apply:TUI 组(src/tui/* + builtin 元数据)/ CLI 组(cli/config/credential/main)改文件不相交,各 worktree 并行,主 agent merge(ff + three-way 无冲突)入单一 change | 主导:用户「一个 change + 拆 prompt」+ 主 agent 编排 | 依据:design ⑤
- 凭据写权限竞态(commit 安全 review MEDIUM):write_credential 原 fs::write(默认 0644)+ 后 chmod 0600,新建/覆盖有 world-readable 窗口含 key 明文 → 改**原子**:同目录 temp(Unix OpenOptionsExt mode 0600 创建)+ fs::rename 替换、去事后 chmod | 选:A 先修再归档(不让缺陷入档)| 弃:B 先归档再 fix change | 主导:安全 review 发现 + 用户选 A | 依据:commit review / code

## 变更
- TUI:tui/{command(元数据+补全)、app(补全 on_key 优先级)、render(布局移底+补全浮层)、terminal(去 capture)、mod(去 MouseEvent)};迁移 snapshots + 新 tui_command_completion
- CLI:cli.rs(run_auth 读全再写 + StdinAuthPrompter 隐藏输入 + AuthPrompter 可注入)、config(write_config merge)、credential(write_credential upsert)、main(auth 分流)
- 安全 fix(commit 3cb8f43,先于本 archive commit):credential write_credential → 同目录 temp + Unix mode 0600 + rename 原子写、去 restrict_permissions;新增 cfg(unix) 测试 write_credential_tightens_permissions_when_overwriting_world_readable_file
- spec:tui-shell ADD(/补全、终端原生复制)+ MODIFY(四区:状态栏移底);builtin-commands MODIFY(命令元数据同源);cli-runtime ADD(auth);credential-source ADD(凭据写入);config-layering ADD(配置写入)
- 验证:两 worktree 各自全绿;fix 后 master 集成 220 unit + 1 e2e 全绿、clippy/fmt/validate 过;TUI ff + AUTH three-way + fix ff merge 无冲突

## 待决
- 7.4 手动冒烟未跑(留用户):/ 补全 / 终端复制 / 状态栏最底 / mysteries auth 配置后进 TUI
- auth 隐藏输入非 panic-safe(read_secret_hidden 闭包后 disable,无 panic 路径故可接受)
- 元数据同源:COMMANDS 单一定义(测试锁同源,低风险)
- 承前未动:git 身份 wanglei30 临时 + leafiellune purge;summary 调用期间无 on_status(20 待决)

## 流程
- propose 主 agent 起草(用户授权直接给 apply prompt);两组并行;3 红灯停点(① TUI 补全 ② CLI 凭据写 ③ CLI auth)全遵守;5.1 曾用 todo!() 桩主 agent 指正 → 6.1 改行为桩;5.3 据复核补 0600 测试
- **主 agent review 盲点 + commit 安全 review 补漏**:5.3 主 agent 信测试绿 + 自跑通过、**未逐行读 write_credential 实现**;测试只验最终 0600、未覆盖创建竞态窗口 → commit 安全 review 发现 MEDIUM → 用户选 A → fix worktree 修 → 主 agent **逐行读 fix 实现**确认原子无窗口。教训:review 绿灯实现须读码,测试绿 ≠ 安全。

## 引用
- change:tui-ux-and-cli-auth(archive 2026-06-28-*)
- 前置:add-token-compaction(20,1.1);本 change 属 1.1.x 完善
- session:本会话——1.1.x 规划(4 点 + 点4 选 A)+ 一个 change 两组并行 apply + 3 红灯停审 + 两组 merge + commit 安全 review 凭据权限 fix(选 A 先修再归档)
- memory:subagents-skip-red-light-stops(本批 dispatch 写硬 → 3 红灯全遵守);review-read-impl-not-just-green-tests(本批新增:5.3 盲点教训)
