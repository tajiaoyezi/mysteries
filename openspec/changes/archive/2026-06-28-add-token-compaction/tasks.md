# Tasks — add-token-compaction

> TDD:config / prepare 签名 / Compacting / 阈值均 headless 内核,**强制红-绿**。
> 🔴 三个**红灯停点**:① `prepare` 签名加 `last_usage`(MODIFY 接口契约)② `Compacting::prepare` 核心(新策略,最关键)③ `/compact` 命令路径。各在测试首次成型、贴出**运行时**失败输出后**停下等确认**,再写绿。
> 红灯构造为「运行时红」:签名 / 类型改动先加最小桩使其编译,断言真实行为 → 运行时失败。

## 1. 压缩配置(config-layering,强制 TDD)

- [x] 1.1 【红】测:`resolve` 默认 `compact_trigger_ratio = 0.8` / `keep_recent_turns = 1` / `model_context_window = None`;TOML 解析 + 两层 merge 覆盖;`ratio` 越界(≤0 或 >1)报配置错。运行确认失败。
- [x] 1.2 【绿】config 加三项 + 默认 + 校验(最小实现)。
- [x] 1.3 零回归:既有 config-layering 测保持绿。

## 2. prepare 签名 + last_usage(context-strategy MODIFY + agent-loop,强制 TDD)

- [x] 2.1 【红】测:`Passthrough::prepare` 带任意 `last_usage` 仍与 history 逐条等价(忽略);`Agent` 多轮时**下一轮** `prepare` 收到的 `last_usage` = 上一轮 `response.usage`,**首轮** `None`。运行确认失败(改签名加参数后用最小桩制造运行时红)。
- [x] 2.2 🔴 **红灯停点①**:贴出 2.1 测试 + 失败输出,**停下等确认**。
- [x] 2.3 【绿】`prepare` 签名加 `last_usage: Option<&Usage>`;`Passthrough` 忽略;`Agent` loop 每轮记 `response.usage`、下轮传入;改全部调用点(含强制收尾那次)。
- [x] 2.4 零回归:既有 agent-loop + context-strategy 测保持绿(Passthrough 等价)。

## 3. Compacting 核心(context-strategy ADD,强制 TDD)

- [x] 3.1 【红】测(Mock provider 脚本含 summary 响应):① 超阈值 `last_usage`(`input_tokens > window × ratio`)→ `prepare` 重写为 `[System(原 system + summary), 最近 keep 轮原文]`,summary 取自 provider;② 未超 / `last_usage = None` → 等价原 history;③ 保留窗口边界对齐 `User`、**不**切断 `tool_calls↔tool_result`;④ summary **入 System**、不新增独立 message;⑤ summary 的 `provider.complete` 失败 → 退回不压(原 history,`Ok`)。运行确认失败。
- [x] 3.2 🔴 **红灯停点②**:贴出 3.1 测试 + 失败输出,**停下等确认**(Compacting 核心、最关键)。
- [x] 3.3 【绿】`Compacting::prepare`:阈值判定 → 末尾向前数 `keep_recent_turns` 个 `User` 定保留边界 → 压缩区间以结构化 prompt(`tools` 空)调 provider 生成 summary → 重写 history(summary 拼入 System)→ 失败退回不压。
- [x] 3.4 边界(连写不停):`keep_recent_turns = 0` 全压;`keep ≥` 现有轮数(无可压区间)等价不压;summary 可被再次压缩(幂等)。

## 4. 手动 /compact(builtin-commands,强制 TDD)

- [x] 4.1 【红】测:`/compact` 解析 + 执行 → 立即对当前 history 压一次(**无视阈值**,复用 `Compacting`);summary 失败回 notice(可重试);压缩禁用 / 无 provider 回提示不 panic。运行确认失败。
- [x] 4.2 🔴 **红灯停点③**:贴出 4.1 测试 + 失败输出,**停下等确认**(新命令路径)。
- [x] 4.3 【绿】`/compact` 命令(解析 + 执行,封装 `Compacting` 立即压)+ notice 回显(压缩前后消息数 / 失败提示)。

## 5. 装配 + provider 共享 + error

- [x] 5.1 `Agent.provider` `Box` → `Arc<dyn Provider>`;`Compacting::new(Arc<provider>, model, cfg)`;cli-runtime / app 据 config(`window` 已配)装 `Compacting`、否则 `Passthrough`。
- [x] 5.2 `ContextError` → `AgentError::Context`(还掉 `add-context-strategy` 临时的 `→ProviderError::Transport`)。
- [x] 5.3 零回归:`Box→Arc` 与装配改动后既有 e2e / cli-runtime / tui 测保持绿。

## 6. 收尾验证

- [x] 6.1 `cargo build` 通过;`cargo test` 全绿(含新红-绿 + 零回归)。
- [x] 6.2 `openspec validate add-token-compaction --strict` 通过。
- [x] 6.3 `cargo clippy --all-targets -- -D warnings` 零警告;`cargo fmt --check` 净。
- [x] 6.4 手动冒烟(非自动,留用户):配 `model_context_window` 后跑长会话触发自动压缩;`/compact` 手动触发;均最终回答可见、不崩。
