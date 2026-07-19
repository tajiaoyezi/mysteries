# 真机验证操作手册 — `add-agent-execution-scope`

> 只验证本 change 的 root cancellation、权限零回归、并行工具收口与 session 恢复。
> 自动化实现与门禁已经完成；本文件及 `tasks.md` §8 只能由用户根据真机结果勾选，实施 Agent 不得代勾。

## 0. 测试对象与准备

在仓库根目录的 PowerShell 7 中执行：

```powershell
$exe = 'H:\devlopment\code\wps\mysteries\target\codex-agent-scope\release\mysteries.exe'
Test-Path -LiteralPath $exe
& $exe --version
& $exe
```

预期：

- `Test-Path` 为 `True`。
- `--version` 正常退出并显示 `mysteries 1.2.0`。
- TUI 正常启动；建议终端宽度至少 80。
- 使用 Shift+Tab 切换 Normal / AcceptEdits / Yolo / Plan。

自动化基线：

- `cargo fmt --all -- --check`：通过。
- `cargo clippy --all-targets --locked -- -D warnings`：通过。
- `cargo test --locked`：958 个 lib test 通过、4 ignored；8 个 e2e test 通过。
- `cargo build --release --locked`：通过。
- OpenSpec strict：change 通过；全仓 19/19。
- RustSec：0 vulnerability、0 unsound；保留既有 `bincode 1.3.3` unmaintained warning。
- Midnight / Daylight Interrupted 快照零变化，无 `.snap.new`。

---

## 勾选总表

```text
[x] 8.1 Normal / AcceptEdits / Yolo / Plan 正常轮零回归
[x] 8.2 Provider 等待中断只有一个终态，下一轮可继续
[x] 8.3 权限框 Esc 与 Executing 阶段 turn cancellation 边界正确
[x] 8.4 并行读取中断全部收口，无迟到 Done
[x] 8.5 --continue / --resume 恢复无 Running 或重复结果
```

---

## 8.1 四种权限模式正常轮

### A. Normal：并行只读

切到 Normal，发送：

```text
必须在同一轮同时调用以下工具，不要串行执行：
1. list_dir 列出 .
2. read_file 读取 Cargo.toml 前 20 行
3. read_file 读取 README.md 前 20 行
4. glob 搜索 **/*.rs
5. grep 在 src 中搜索 AgentExecutionScope
不要调用其他工具。全部完成后简短总结。
```

通过标准：

- 不出现权限框。
- 多个本地读取可同时出现 Running 卡；超过 4 个时仍保持并发上限 4。
- 结果按模型 tool call occurrence 顺序收口，最终回答正常。

### B. Normal：Network preview

发送：

```text
必须调用 web_fetch 获取 https://example.com/，不要调用其他工具。
```

通过标准：

- 出现 Network 权限框，包含完整 args、canonical target 与 redirect/SSRF scope。
- 按 `n` 拒绝后仅当前调用被拒绝，TUI 仍可继续。

### C. AcceptEdits

切到 AcceptEdits，发送：

```text
必须调用 write_file 创建 agent-scope-accept-edits-smoke.txt，内容严格为 accept-edits-ok；完成后简短回答。
```

通过标准：

- Edit 自动放行并创建文件。
- 再要求执行 `echo execute-still-prompts` 时，Execute 仍出现权限框；按 `n` 拒绝即可。

### D. Yolo

切到 Yolo，依次发送：

```text
必须调用 run_shell 执行完全一致的命令：echo agent-scope-yolo-ok
```

```text
必须调用 web_fetch 获取 https://example.com/，不要调用其他工具。
```

通过标准：

- 合法 Execute 与可授权 Network preview 自动放行。
- 工具结果和最终回答正常。
- 畸形或 reject-only Network 请求仍不能被 Yolo 绕过。

### E. Plan

切到 Plan，发送：

```text
只读分析 README.md 和 Cargo.toml，然后提交一个两步计划；不得写文件、执行命令或联网。
```

通过标准：

- 只读工具和 Plan 工具正常。
- Edit / Execute 不可执行。
- 工具卡、计划面板与最终回答无异常。

退出 TUI 后清理本项临时文件：

```powershell
Remove-Item -LiteralPath '.\agent-scope-accept-edits-smoke.txt' -ErrorAction SilentlyContinue
```

结果记录：

```text
Normal 并行只读：通过。
Normal Network preview：通过。
AcceptEdits：通过。
Yolo：通过。
Plan：通过。
失败项、截图或备注：用户于 2026-07-18 确认全部通过。
```

**本项勾选：** `[x] 8.1`

---

## 8.2 Provider 等待中断

1. 启动 TUI，清空 modal、文本选区和消息队列。
2. 选择响应相对较慢的模型/思考档位。
3. 发送：

```text
不要调用任何工具。请先深入分析本项目 Agent execution scope 的 cancellation、budget、capability 三个维度，再输出不少于 20 条审查结论。
```

4. 在仍显示 CallingModel / 思考且工具尚未开始时按 Esc。
5. 看到终态后立即发送：

```text
只回复 PROVIDER-INTERRUPT-RECOVERY-OK
```

通过标准：

- 只出现一次 `⊘ 已中断本轮`。
- 中断后没有迟到文本、usage、Idle 或第二个 terminal event。
- 下一条 Prompt 正常返回 `PROVIDER-INTERRUPT-RECOVERY-OK`。
- 下一条 Prompt 不得继续分析旧任务；旧 Prompt 只保留在可见 transcript，不再进入下一轮模型 history。
- 如果模型在 Esc 前已经完整完成而显示正常 TurnComplete，这是 completion 赢得竞态，不算中断验证；重新执行本项。

结果记录：

```text
唯一 Interrupted：通过，仅出现一次 `⊘ 已中断本轮`。
无迟到事件：通过，未观察到旧任务迟到文本或第二个终态。
下一轮恢复：通过，只返回 `PROVIDER-INTERRUPT-RECOVERY-OK`。
失败项、截图或备注：2026-07-18 首次验证发现下一轮 Provider 仍携带旧 Prompt并继续输出旧任务；新增未提交 User turn 回滚与自动化回归后，用户使用 17:47 构建的 release binary 复测通过。
```

**本项勾选：** `[x] 8.2`

---

## 8.3 permission 与串行 Executing 中断

### A. 权限框内 Esc 只拒绝当前调用

切到 Normal，发送：

```text
必须调用 run_shell 执行完全一致的命令：echo permission-modal-escape-test
```

权限框出现后按 Esc。

通过标准：

- 当前 ToolCall 被拒绝。
- 不出现整轮 `⊘ 已中断本轮`。
- TUI 仍可立即接收下一条 Prompt。

### B. modal 关闭后的 turn cancellation

发送：

```text
必须调用 run_shell 执行完全一致的命令：ping 127.0.0.1 -n 8 >NUL && echo AGENT_SCOPE_DELAY_DONE
```

1. 权限框出现后按 `y`。
2. 等权限框关闭、工具卡进入 Executing / Running 后按 Esc。
3. 立即发送：

```text
只回复 SERIAL-INTERRUPT-RECOVERY-OK
```

通过标准：

- 只出现一次 `⊘ 已中断本轮`。
- 当前工具卡收口为 Error / `工具调用已中断`，不出现迟到 Done。
- 下一轮正常返回 `SERIAL-INTERRUPT-RECOVERY-OK`。
- 本项只验证 Agent future/history/observer 收口；不宣称已启动的 OS 进程被强制终止。

结果记录：

```text
modal 内 Esc：通过。
Executing 后 Esc：通过。
无迟到 Done：通过。
下一轮恢复：通过。
失败项、截图或备注：用户于 2026-07-18 确认全部通过。
```

**本项勾选：** `[x] 8.3`

---

## 8.4 并行读取中断

1. 保持 Normal，清空 modal、选区和队列。
2. 发送以下 Prompt；路径中的 `devlopment` 拼写不要修改：

```text
必须在同一轮同时发起以下 5 个 grep，不要串行执行。每个 grep 的 path 都严格设置为 H:\devlopment\code：
1. pattern：AGENT_SCOPE_INTERRUPT_NEVER_MATCH_01
2. pattern：AGENT_SCOPE_INTERRUPT_NEVER_MATCH_02
3. pattern：AGENT_SCOPE_INTERRUPT_NEVER_MATCH_03
4. pattern：AGENT_SCOPE_INTERRUPT_NEVER_MATCH_04
5. pattern：AGENT_SCOPE_INTERRUPT_NEVER_MATCH_05
不要调用其他工具，全部完成后再回答。
```

3. 看到多张 Running 卡后立即按 Esc。
4. 立即发送：

```text
中断恢复探测：只调用 read_file 读取 H:\devlopment\code\wps\mysteries\Cargo.toml 前 10 行，然后回答 PARALLEL-INTERRUPT-RECOVERY-OK。
```

通过标准：

- 所有未完成卡片收口为 Error；已在中断前发布的 Done 保持 Done。
- 只出现一次 `⊘ 已中断本轮`。
- 后台读取自然结束后不产生迟到 Done / finished / Idle。
- 恢复 Prompt 正常完成，没有 dangling tool result 或 Provider 协议错误。

如果 5 个工具在来得及按 Esc 前全部结束，本轮不算失败，也不算完成；重新执行或扩大扫描路径。

结果记录：

```text
多卡收口：通过。
唯一 Interrupted：通过。
无迟到 Done：通过。
下一轮恢复：通过。
失败项、截图或备注：用户于 2026-07-18 确认全部通过。
```

**本项勾选：** `[x] 8.4`

---

## 8.5 Session 恢复

本项以前面 8.2–8.4 中已经成功中断的 session 为基础。

### A. `--continue`

1. 正常退出 TUI。
2. 在相同仓库目录执行：

```powershell
& $exe --continue
```

3. 恢复后发送：

```text
只调用 read_file 读取 Cargo.toml 前 10 行，然后回答 CONTINUE-SCOPE-OK。
```

通过标准：

- 直接恢复最近 session。
- 没有 Running 卡残留。
- 每个旧 tool call occurrence 最多一个 interrupted result，无重复补齐。
- 首轮 Provider / 工具正常并回答 `CONTINUE-SCOPE-OK`。

### B. picker `--resume`

1. 再次正常退出。
2. 执行：

```powershell
& $exe --resume
```

3. 在 picker 中选择同一个 session，发送：

```text
只调用 read_file 读取 README.md 前 10 行，然后回答 RESUME-SCOPE-OK。
```

通过标准：

- picker 选中后恢复正确 session。
- 没有 Running 残留、重复 interrupted result 或历史卡片被新 occurrence 错改。
- 首轮 Provider / 工具正常并回答 `RESUME-SCOPE-OK`。

结果记录：

```text
--continue：通过。
--resume：通过。
无 Running：通过。
无重复 interrupted result：通过。
失败项、截图或备注：用户于 2026-07-18 确认全部通过。
```

**本项勾选：** `[x] 8.5`

---

## 全部通过后

1. 把本文件的 8.1–8.5 和总表勾为 `[x]`。
2. 把 `tasks.md` §8.1–8.5 同步勾为 `[x]`。
3. 将真机结果发给主 Agent；主 Agent复核后判断是否可以进入 archive。

失败时至少提供：步骤编号、权限模式、实际 Prompt、实际文案/截图、是否稳定复现。
