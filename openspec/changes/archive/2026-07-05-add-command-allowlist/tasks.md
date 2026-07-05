# Tasks — add-command-allowlist

红灯纪律:红灯独立成步,以断言失败落红(非编译错)——新类型/新签名允许红灯内先落桩。**红灯停点**:3.1 为**新权限路径契约**(`PermissionReply` + `PermissionRequest` 改造 + `decide` 三段判定)首次成型,测试 + 失败输出贴出后**停下等确认**再进绿灯;1.x / 2.x(纯 kernel:PolicyEngine / config)可连写。
执行 agent MUST NOT:git 写操作、修改既有快照/夹具以过测(**例外见 3.1**:responder 契约从 `PermissionDecision` 换 `PermissionReply` 令既有 channel 测试合法翻红,须更新而非绕过)、勾选第 6 节真机任务、**全仓 `cargo fmt`**(只碰你改的文件)、kill 用户进程。

## 1. PolicyEngine 纯逻辑(强制 TDD)

- [x] 1.1 红→绿:`PolicyEngine { allowed: BTreeSet<String> }` + `from_commands(iter)`;`normalize(cmd) -> String`(free/assoc fn、无 `&self` = `split_whitespace().collect::<Vec<_>>().join(" ")`);`permission_key(call, tool) -> Option<String>`(assoc fn、无 `&self`;**仅 `tool.permission_level() == Execute`** 且 `call.arguments["command"]` 为 string → `normalize`,否则 `None`);`is_allowed(&self, call, tool) -> bool` = `permission_key(...).is_some_and(|k| self.allowed.contains(&k))`;`remember(&mut self, key)`。测试:`normalize` 压空白+trim、`permission_key` 对 `Execute`+`command` 规范化、**非 `Execute` 级或无 `command` → `None`**、`is_allowed` 命中/未命中、`remember` 后命中、`from_commands` dedup。`PermissionDecision`/`gate`/`auto_allows` **不动**。

## 2. config allowed_commands(强制 TDD)

- [x] 2.1 红→绿:`RawConfig.allowed_commands: Option<Vec<String>>`(`#[serde(default)]`);resolve 时 `Config.allowed_commands: Vec<String>`(`unwrap_or_default`);两层 merge = **并集 dedup**(**显式例外**于字段级 override,审查 D5);`append_allowed_command(path, cmd) -> Result<(), ConfigError>`(read-modify-write、dedup,仿 `write_config`)。测试:TOML 解析 `allowed_commands`、缺省 → 空 `Vec`、user∪project 并集去重、append 去重且落盘可再读、append 到无该字段的 config。

## 3. PermissionReply + ChannelDecider 三段判定(强制 TDD)

- [x] 3.1 红(**停点**):`PermissionReply { AllowOnce, AllowAlways, Deny }`(`src/permission/`);`PermissionRequest` 加 `allow_always_key: Option<String>`、`responder: oneshot::Sender<PermissionReply>`(**更新既有 channel 测试** `send(PermissionDecision::Allow)` → `send(PermissionReply::AllowOnce)`——合法契约变更、豁免「禁改夹具」);`ChannelDecider::new` 加 `policy: Mutex<PolicyEngine>` + `user_config_path`;`decide` 三段判定签名成型、桩实现令测试断言红。测试(断言红):allowlist 命中不发 channel 直 `Allow`、未命中 `Normal` 发 channel、回送 `AllowOnce` → `Allow`、responder drop → `Deny`。**贴测试 + 失败输出,停下等确认。**
- [x] 3.2 绿:`decide` 三段完整——①`is_allowed`→`Allow`(不发 channel)②`auto_allows(mode,level)`→`Allow`③发 `PermissionRequired{tool_name,args,allow_always_key}` 等 `PermissionReply`:`AllowOnce→Allow`、`AllowAlways`→`remember`+`append_allowed_command`(失败经既有 `tx` 发 `Notice`)+`Allow`、`Deny`/断开→`Deny`。**锁不跨 `.await` 持有**(取 key/查成员即释放锁,再 `await` oneshot)。补测试:`AllowAlways` 落盘+内存 remember(后续同命令不发 channel)、持久化失败仍 `Allow`+`Notice`、key `None` 时 `AllowAlways` 退化 `Allow` 不落盘。

## 4. TUI 权限框接线(IO 胶水,事后回归)

- [x] 4.1 权限框 `[a·总是允许]` 选项:`render.rs` C6 框在 `pending_permission` 的 `allow_always_key.is_some()` 时加 `[a·总是允许]`(keyless 时**不显**、既有快照零 churn);`app.rs` `pending_permission` 键处理加 `a`(仅 key 存在)→ 送 `PermissionReply::AllowAlways`,`y`→`AllowOnce`、`n`/`Esc`→`Deny`。**补 insta 快照**:命令类工具(带 `command`)权限框含 `[a·总是允许]`。
- [x] 4.2 装配:构造 `ChannelDecider` 处(`assemble_agent` / run_tui 装配)从 `config.allowed_commands` 建 `PolicyEngine::from_commands`、连同 `paths.user_config` 注入 `ChannelDecider::new`。`--headless` 保持既有 decider、不受影响(always-allow 为 TUI 交互特性)。
- [x] 4.3 权限框 diff 按框高截断(**附带修既有 render bug**、保动作行可见):`render_permission`(render.rs)diff body 无 cap → 长 diff 把 `[y·允许][n·拒绝]` 动作行挤出框外裁掉。改:固定行 = 5(标题/tool/args/动作/提示)+ 边框 2 = 7;`budget = area.height.saturating_sub(7) as usize`;全量 diff 行数 > budget → 取前 `budget.saturating_sub(1)` 行 + 末行「⋯ 其余 N 行」(warning_bg 仿 diff 区),否则全量;动作/提示行照旧在其后 extend。`budget` 极小(0)时至少截到只剩「⋯」、不 panic。**补 insta 快照**:长 diff 的 `write_file` 权限框断言含「⋯ 其余 N 行」+ 动作行 `[y·允许][n·拒绝]` 完整可见;既有短 diff 权限框快照**零 churn**。(事后回归、非 allowlist 逻辑)

## 5. 门禁

- [x] 5.1 `cargo test --lib` 全绿(含 3.1 更新后的 channel 测试 + 4.3 权限框截断快照);`cargo clippy --all-targets -- -D warnings` 零警告;快照仅预期新增、既有零 churn
- [x] 5.2 `openspec validate add-command-allowlist --strict` 通过
- [x] 5.3 `cargo build` 通过(exe 被占报 os error 5 即报告、别 kill)

## 6. 真机核验(主 agent / 用户;执行 agent MUST NOT 勾)

- [x] 6.1 Normal 模式:agent 跑一条 shell 命令 → 弹权限框含 `[y·允许][a·总是允许][n·拒绝]`;按 `a` → 执行 + 之后**同命令不再问**
- [x] 6.2 持久化:退出重启(冷启动,无 flag)→ 上次 `a` 的命令**仍自动放行**(已写入 user config `allowed_commands`)
- [x] 6.3 精确匹配:`a` 允许 `git status` 后,`git status -s` **仍会问**(精确匹配、非前缀)
- [x] 6.4 回退:`allowed_commands` 空 + Edit 类工具 → 权限框**无** `[a]`(无 command key);行为与现状一致
