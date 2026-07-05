# Design — add-command-allowlist

## 决策

### D1 PolicyEngine 接在 `ChannelDecider::decide` 最前(gate/agent-loop 零改)
- **现状**([channel.rs:125](src/tui/channel.rs)、[agent/mod.rs:186](src/agent/mod.rs)):agent-loop 调 `gate(call, tool, decider)`;`gate` 对 `ReadOnly` 直放、否则 `decider.decide()`;`ChannelDecider::decide` 先 `auto_allows(mode, level)` 命中即 `Allow`,否则发 `PermissionRequired` oneshot 等 `PermissionDecision`。
- **选**:PolicyEngine 放进 `ChannelDecider`(具体 decider),`gate()` 与 agent-loop **不动**(与 add-token-compaction 等一样,经既有 `PermissionDecider` seam 接入)。`decide` 三段判定:
  1. `policy.is_allowed(call, tool)` → `Allow`(allowlist 命中,**先于 mode**——精确命令白名单比模式更具体);
  2. `auto_allows(mode, level)` → `Allow`(既有 mode 短路);
  3. 都不中 → 发 `PermissionRequired`(带 permission key)等 `PermissionReply`。
- **弃**:改 `gate()` 签名注入 policy(要动 agent-loop 调用点 + 其 Mock 测试,面更大);把 policy 逻辑写进 agent-loop(污染 headless 核、且 always-allow 天然涉 UI 回复)。

### D2 permission key = 命令规范化串,仅命令类工具(v1)
- **选**:`PolicyEngine::permission_key(call, tool) -> Option<String>` = **仅当 `tool.permission_level() == Execute`** 且 `call.arguments["command"]` 为 string 时 → `normalize`(trim + 内部连续空白压成单空格);否则 `None`(`Edit`/无 `command`/`ReadOnly` 均不产 key——`tool` 参数用于 level 判别,防带 `command` 字段的非执行类工具被误 allowlist)。`is_allowed` = `permission_key(...).is_some_and(|k| self.allowed.contains(&k))`。`allowed` 为 `BTreeSet<String>`(dedup + 有序,便于测试与写回稳定)。
- **精确匹配(取舍,审查点)**:`git status` 与 `git status -s`、`cargo build` 与 `cargo build -q` 算**两条**独立白名单项。理由:精确匹配**安全**(不会因 `git` 前缀误放 `git push --force`)、实现/测试确定、无歧义。**prefix / glob 显式留后续 change**(要引入匹配语法 + 安全边界,不塞 v1)。
- **仅命令类(取舍)**:v1 key 只从 `command` 字段派生 → 事实上只覆盖 shell/`Execute`。`Edit`(`path` 参数)无 key、不给 always-allow——`AcceptEdits` 模式已覆盖「放行所有编辑」的钝需求;**per-path 编辑白名单留后续**。不硬编码工具名,靠 `command` 字段存在与否判别(健壮)。
- **normalize 纯函数**(强制 TDD):`"git   status"` → `"git status"`;首尾空白去除;空串/仅空白 → 空串(不入白名单)。

### D3 弹窗回复升 `PermissionReply`,`PermissionDecision` 不变
- **选**:新增 `PermissionReply { AllowOnce, AllowAlways, Deny }`(UI → decider 的 oneshot 回复);`PermissionRequest.responder` 由 `Sender<PermissionDecision>` 换 `Sender<PermissionReply>`,请求加 `allow_always_key: Option<String>`(`Some` → UI 显 `[a·总是允许]`)。`PermissionDecision { Allow, Deny }` **保持不变**——`gate()` 返回、agent-loop 判 `== Deny` 全不动。`decide` 内部映射:`AllowOnce → Allow`;`AllowAlways →`(记忆 + 持久化)`→ Allow`;`Deny`/responder 断开 → `Deny`(fail-safe 不变)。
- **弃**:给 `PermissionDecision` 加 `AllowAlways` 变体(会波及 `gate()`/agent-loop 的所有 `PermissionDecision` match + 语义污染「门只判 Allow/Deny」);另开一条 side channel 传「记住」信号(双信号更复杂)。

### D4 always-allow 落盘 = 内存记忆 + 写 user config
- **选**:`decide` 收到 `AllowAlways` 且 key `Some(k)`:
  1. `policy.remember(k.clone())`(内存 `allowed.insert`,本会话即时生效);
  2. `append_allowed_command(&self.user_config_path, &k)`(config 写回,dedup);
  3. 返回 `Allow`。
  - key `None`(不该发生,UI 仅在 `Some` 时给该选项)→ 退化为 `AllowOnce`(`Allow`、不落盘)。
  - **持久化失败**(fs 错):`decide` 经既有 `tx` 发 `AgentEvent::Notice("命令白名单持久化失败:…")`——**非致命**,内存记忆仍在(本会话不再问),仅跨 run 未留。不 panic、不改判定结果(仍 `Allow`)。
- **`append_allowed_command(path, cmd)`**(config/mod.rs 新增,仿 `write_config` 的 read-modify-write):`read_raw_config` → `allowed_commands` 去重加入 → `toml::to_string_pretty` → `fs::write`。sync 小写入(与既有 `write_config` 同步风格一致;在 async `decide` 内直调,写入极小可接受)。
- **`Arc<Mutex<PolicyEngine>>` vs `Mutex`**:`decide` 为 `&self`(async_trait),`remember` 需内部可变 → decider 持 `Mutex<PolicyEngine>`;仅 decider 触及,v1 **不必 Arc**。锁只在同步取/插时短持,**不跨 `.await` 持锁**(判定读一次 key/成员即释放,再 `await` oneshot)。

### D5 config `allowed_commands` 并集 merge(信任叠加)
- **选**:`RawConfig.allowed_commands: Option<Vec<String>>`(`#[serde(default)]`);resolve → `Config.allowed_commands: Vec<String>`(`unwrap_or_default`)。user/project **并集**(dedup):project 也可预置只读白名单(如仓库约定的 `cargo test`),user 为个人 always-allow 累积——两者**都信任、取并**,而非字段级 override(override 会让 project 抹掉 user 的 always-allow,语义错)。always-allow **只写 user 层**。
- **弃**:沿用字段级 override(`add-multi-provider-config` 的 profiles 是 map-merge、标量是 override;allowlist 是**集合语义**,并集才对)。承认这是对 `config-layering`「字段级 override」通则的**显式例外**,须 spec scenario + 测试锁定。

## 接缝(实现挂载点)
- **PolicyEngine / PermissionReply**:`src/permission/`(新 `policy.rs` 或并入 `mod.rs`);kernel、强制 TDD。`PermissionDecision`/`gate`/`auto_allows` 不动。
- **decide 三段 + 落盘**:[channel.rs:125](src/tui/channel.rs) `ChannelDecider::decide`;`::new` 加 `policy` + `user_config_path` 参;`PermissionRequest` 加 `allow_always_key`、responder 换 `PermissionReply`(既有 channel 测试 `send(PermissionDecision::Allow)` → `send(PermissionReply::AllowOnce)`,合法契约变更、非造假过测)。
- **config**:`src/config/mod.rs` `RawConfig`/resolve/merge(并集)/`append_allowed_command`。
- **TUI 权限框**:`src/tui/app.rs` 既有 `pending_permission` 的 y/n 处理加 `a`(仅 key 存在时)→ 送 `AllowAlways`;`src/tui/render.rs` C6 框在 key 存在时加 `[a·总是允许]`;新快照一张(命令类工具权限框),既有 keyless 权限框快照**零 churn**(无 key → 不显 `[a]`)。
- **装配**:构造 `ChannelDecider` 处(`assemble_agent` / run_tui 装配)从 `config.allowed_commands` 建 `PolicyEngine`、连同 `paths.user_config` 注入。

## 风险 / 权衡
- **精确匹配可能偏窄**:每个 arg 变体要各自 always-allow 一次。接受为 v1 安全默认;prefix/glob 留后续。
- **sync 写入在 async decide 内**:写极小(几行 TOML),与既有 `write_config` 同步风格一致;若日后成本高再 `spawn_blocking`。
- **并集 merge 的例外**:与 config-layering 通则不同,须显式 spec + 测试,避免后人误按 override 改。
- **user_config_path 注入**:decider 多一个路径依赖;headless/测试用临时路径,不依赖真实 config。

## 定案(待用户 review 时确认的细粒度点)
1. permission key = **精确规范化命令串**、**仅命令类(shell/Execute)**;prefix/glob 与 per-path 编辑白名单留后续。
2. 弹窗回复 = **`PermissionReply{AllowOnce,AllowAlways,Deny}`**;`PermissionDecision` 不变。
3. always-allow **写 user 层**;merge = **并集**。
