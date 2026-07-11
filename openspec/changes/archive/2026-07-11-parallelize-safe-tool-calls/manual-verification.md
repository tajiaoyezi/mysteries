# 真机验证操作手册 — `parallelize-safe-tool-calls`

> 只验证本 change 的并行工具 / 中断 / 恢复行为。
> 建议在仓库根目录执行，用 **已配置好 API 的** 本机二进制。
> 自动化门禁通过后，由用户勾选；主 agent / 自动实施 agent **不得代勾**。

**启动（二选一）：**

```bash
# 已 cargo install
mysteries

# 或本仓库
cargo run --release
```

**准备：**

- 权限模式用 **Normal**（Shift+Tab 可切换，先确认在 Normal）
- 输入框上方能看到活动状态行
- 建议终端宽度 ≥ 80

勾选状态与 `tasks.md` §10 对应；本文件给出**具体操作**与**预期效果**。

---

## 勾选总表

```
[x] 10.1  并行读取文案 + 多 Running 卡 + 顺序正确
[x] 10.2  读/写屏障 + Normal/AcceptEdits/Yolo/Plan + 单 modal
[x] 10.3  Esc/Ctrl+C 中断收口 + 一次 notice + 可续聊
[x] 10.4  headless 多读成功；y/n 与 Network 拒绝行为正常
[x] 10.5  连批响应；--continue / --resume 无 Running 残留
```

全部通过后：在 `tasks.md` 将 10.1–10.5 勾为 `[x]`，再发起 `/opsx:archive`。

---

## 10.1 并行本地读取（TUI Normal）

### 你要做的

1. 启动 TUI：`mysteries`（或 `cargo run --release`）
2. 确认模式为 **Normal**
3. 发送（可复制）：

```text
请在本仓库同一轮回复里，同时调用多个工具（不要一个个串着问我），至少做这些：
1) list_dir 路径为 .
2) glob 找 **/*.rs
3) grep 搜索 pattern 为 ToolConcurrency 或 ParallelSafe
4) read_file 读取 src/tool/mod.rs 前 40 行
不要写文件、不要 run_shell、不要联网。做完用一句话总结找到了什么。
```

4. 观察工具执行阶段的 **活动状态行** 和 **ToolCard**
5. （可选加强）若模型只调了 2 个，再发：

```text
再来一轮：同轮并行至少 5 次 read_file，分别读 src/tool/mod.rs、src/agent/mod.rs、src/tui/app.rs、src/tui/mod.rs、README.md 的开头几行。
```

### 预期效果

| 观察点 | 预期 |
|--------|------|
| 权限 | **不弹** 授权框（只读工具） |
| 2–4 个工具同批 | 活动行类似：`⠋ 并行执行 N 个工具… · esc 中断` |
| >4 个工具同批 | 活动行类似：`⠋ 处理 N 个工具（最多并行 4）…`（**不是**「并行执行 5」） |
| 工具卡 | 同批可同时出现 **多张 Running** 卡 |
| 顺序 | 最终 Done/结果顺序 = 模型发出的 tool_calls 顺序（occurrence），不是「谁先跑完谁先显示完成」乱序 |
| 结束 | 有正常最终回答；卡都非 Running |

**不合格：** 只有「执行 read_file…」单名状态却明明有多个并行卡；或 >4 时写「并行执行 5 个工具」。

**本项勾选：** `[x] 10.1`

---

## 10.2 屏障 + 权限回归

### 你要做的

#### A. 屏障（读不得跨过写/执行/网）

1. 仍在 **Normal**
2. 发送：

```text
同一轮里请按这个顺序调用工具（保持顺序，不要重排）：
1) read_file：src/tool/mod.rs 前 20 行
2) grep：pattern 为 MAX_PARALLEL
3) write_file：写到 tmp-parallel-smoke.txt，内容为 smoke-test
4) list_dir：.
不要跳过 write_file。写完再 list。
```

3. 盯住工具卡启动顺序：在 `write_file` 弹出权限并处理 **之前**，后面的 `list_dir` **不应** 已变成 Running/Done

#### B. Normal 权限

4. 对上面的 `write_file`：应出现 **一个** 权限 modal（diff / 确认）
5. 先 **拒绝** 一次，看流程是否继续并写入拒绝结果
6. 再发一轮要求 `write_file` 或 `run_shell`，**允许** 一次，确认可执行

#### C. 模式切换（快速抽查）

| 操作 | 你要做的 | 预期 |
|------|----------|------|
| AcceptEdits | Shift+Tab 切到 AcceptEdits，再让模型 `write_file` 到临时文件 | Edit **可自动放行**；`run_shell` 仍要确认 |
| Yolo | 切到 Yolo，再试 `write_file` / 简单 `run_shell` | Edit/Execute 可自动放行（仍注意安全） |
| Plan | 切到 Plan，让模型只做调研 + 可选 `submit_plan` | **不能** 真写文件/跑 shell；Edit/Execute 被拒或不可用 |
| Network | Normal 下让模型 `web_fetch` 一个公开 URL（如 example.com） | 出现 **Network 权限** modal（不是只读自动跑） |
| 单 modal | 任何时候 | 屏幕上 **只有一个** pending 授权框，不会叠两个 |

### 预期效果（汇总）

- `read`/`grep` 批次结束后，才到 `write_file` 权限
- `write_file` **之后** 的 `list_dir` 才开始（不跨屏障）
- Normal：写/执行/网仍要确认
- 四种 mode 行为与改前一致；始终单 modal

**测完可删：** 仓库里的 `tmp-parallel-smoke.txt`（若已创建）

**本项勾选：** `[x] 10.2`

---

## 10.3 Interrupt（中断并行批次）

### 你要做的

1. **清场**：无文本选区、无权限/计划/提问 modal、消息队列为空
2. 发送（尽量慢一点的大扫描）：

```text
同轮并行：
1) grep 整个仓库 pattern 为 .（或 fn | struct | impl，让结果多）
2) glob **/*
3) list_dir .
先不要给最终总结，先把工具跑起来。
```

3. 看到 **多张 Running** 卡 + 并行活动行后，立刻：
   - 按 **Esc**，或
   - **Ctrl+C**（注意：若进入「再按一次退出」提示，只中断本轮即可，不要连按退出程序）
4. 看 Running 卡与 notice
5. **马上** 再发一条：

```text
中断后的探测：只 read_file 读 Cargo.toml 前 15 行，确认你能继续。
```

### 预期效果

| 观察点 | 预期 |
|--------|------|
| Running 卡 | **全部** 变为 Error |
| 卡上 output | `工具调用已中断` |
| 已完成的卡 | 若中断前已有 Done，**保持 Done** 不变 |
| notice | **仅一次** `⊘ 已中断本轮`（不要两条） |
| 后续 Prompt | 能正常 CallingModel → 工具/回答；**无** Provider dangling tool_call 类报错 |
| 进程 | 仍存活，不退出 |

**不合格：** 卡一直 Running；notice 两条；下一轮协议错误。

**本项勾选：** `[x] 10.3`

---

## 10.4 `--headless`

### 你要做的

在项目根目录：

```bash
# 多本地读取（应无交互，直接结束）
mysteries --headless "同轮并行：list_dir .、glob **/*.md、read_file README.md 前 30 行。简短总结。"

# 或
cargo run --release -- --headless "同轮并行：list_dir .、glob **/*.md、read_file README.md 前 30 行。简短总结。"
```

**权限抽查（会交互 y/n）：**

```bash
mysteries --headless "用 write_file 在当前目录写 headless-smoke.txt，内容 hello；然后 read_file 它。"
```

- 出现写文件确认时：先答 **n**，看是否拒绝并继续/结束合理
- 再跑一遍答 **y**，文件应写成功

**Network reject-only 抽查（可选）：**

```bash
mysteries --headless "web_fetch 一个明显非法的地址，比如 http://127.0.0.1/"
```

- 应拒绝/报错，**不应** 静默成功访问内网

### 预期效果

| 场景 | 预期 |
|------|------|
| 多只读 headless | 有正确文字回答；进程 **exit 0** 结束；中途无 TUI |
| write y/n | 与改前一致：n=拒绝，y=写入 |
| 非法/SSRF 类 Network | 拒绝或 is_error，行为不比改前更松 |

**测完可删：** `headless-smoke.txt`（若已创建）

**本项勾选：** `[x] 10.4`

---

## 10.5 性能 / 连续中断 / Session 恢复

### A. 响应与连批

1. 开 TUI，连发 2–3 轮「同轮多 read/grep」
2. 某一轮 Running 时 **Esc 中断**
3. **立刻** 再发下一轮多读取

**预期：**

- spinner / 输入框 / Esc 仍跟手
- 不卡死、不风扇狂转、磁盘无明显失控
- 次批能正常跑完

### B. `--continue` 恢复

1. TUI 里随便聊几轮（含工具），让 session 落盘
2. **退出** mysteries
3. 在同一目录：

```bash
mysteries --continue
```

**预期：**

- 直接进入最近会话（不弹 picker）
- transcript **没有** 永久 Running 卡
- 若上次中断过，历史相关卡应为 Error，文案含 **`上次会话已中断`**（激活规范化）
- 再发一条 Prompt 正常，无 dangling 协议错误

### C. `--resume` picker

1. 退出后：

```bash
mysteries --resume
```

2. 在列表里 **选中** 刚才的会话（Enter）
3. 再发一条带工具的 Prompt

**预期：**

- 选中后 hot-swap 到该会话
- 无 Running 残留
- 新 turn 工具卡正常；即使 `call_id` 复用，也只更新 **新** 卡，不改历史 Error/Done 卡
- **Esc 取消 picker** 时：保持进入时的新会话，不误加载

### D. 对照（可选）

- 中断后不退出，只新 Prompt → 应同 10.3
- 退出再 `--continue` → 磁盘上的 Running 应在激活时被收口

**本项勾选：** `[x] 10.5`

---

## 快速对照表

| ID | 焦点 | 关键文案 / 现象 |
|----|------|----------------|
| 10.1 | 并行读取 | `并行执行 N…` / `处理 N…（最多并行 4）` |
| 10.2 | 屏障+权限 | 读不跨写/执行/网；单 modal |
| 10.3 | 中断 | Error + `工具调用已中断`；一次 notice |
| 10.4 | headless | 多读成功；y/n 不变 |
| 10.5 | 稳定+恢复 | 响应；continue/resume 无 Running 残留 |

---

## 失败时请记录

任一步「实际 ≠ 预期」时记下：

1. 步骤编号（10.x）
2. 权限模式（Normal / …）
3. 使用的 prompt / 命令
4. 实际看到的文案或截图
5. 是否可稳定复现

---

## 全部通过后

1. 在本文件总表与 `tasks.md` §10 同步勾选 `[x]`
2. 发起 `/opsx:archive` 归档本 change
