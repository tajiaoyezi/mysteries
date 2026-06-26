# CLAUDE.md — Mysteries Agent

---

## 全局约定

### 语言
- 对话、注释、文档、commit message 正文用**简体中文**。
- **技术词保留英文原文**:代码标识符、命令、库/框架名、协议名、文件路径、报错信息一律不翻译、不夹译(如 `tokio`、`Provider` trait、`oneshot`、`tool_calls`)。
- commit 首行用 Conventional Commits 英文前缀(`feat:`/`fix:`/`refactor:`/`test:`/`docs:` …),其后描述可中文。

### 技术栈边界
- 实现语言 **Rust**,不引入其他实现语言。
- 核心能力(Agent Loop、工具系统、权限控制、会话管理)**必须自行实现**,禁止引入第三方 Agent SDK / Framework 替代;协议接入、TUI、HTTP、配置、日志、测试可用三方库。
- 新增依赖前先说明理由,不擅自扩张 dependency。

### 工作方式
- 遵循 OpenSpec 流程:先 propose / spec,再实现,最后 archive。不跳过直接写大段代码。
- 任何破坏性操作(文件覆盖/删除、`git` 写操作、shell 执行)**先说明再动手**——与产品自身的权限模型一致。
- 设计选择不确定时,先给选项与取舍,由用户拍板,不自行假设推进。

### 质量基线
- 提交代码必须 `cargo build` 通过;核心逻辑改动须带测试(见下方 TDD)。
- **权威次序:code / 编译器 / 测试 > spec > Agent 推断**。三者冲突时以前者为准,并显式指出冲突,不得静默选一边。

---

## TDD(测试驱动)

### 适用范围(先判断,再决定是否 TDD)
- **强制 TDD —— headless 内核**:Agent Loop、工具系统、权限门、Provider 归一化、配置 merge。纯逻辑、IO 无关、Mock 可驱动者,一律先测后码。
- **不先写测试 —— TUI 外壳**:ratatui 渲染、布局、交互。用 `TestBackend` + 快照(`insta`)做**事后**回归,不走 red-green。
- 拿不准属于哪半,先问,不默认。

### 循环(红灯独立成步,防止把测试照实现反推)
1. **红**:先只写测试,运行确认其**失败**(失败原因正确,非编译错),将测试代码 + 失败输出贴给用户。
   - 停点(当前为折中档):**新 trait / 新工具 / 新权限路径**等接口首次成型时,贴出后**停下等确认**;给已测行为补边界 case 可连写不停。
   - *(改严格档:把上一行改为「任何测试写完后均停下等确认」。)*
2. **绿**:写**最小**实现让测试通过,不提前加未被测试要求的功能。
3. **重构**:测试保持绿的前提下清理,不改外部行为。

### 约束
- 不得在同一步同时产出测试与实现;不得为过测试写退化/造假实现(硬编码返回值、吞错等)。
- 一个行为一组测试,覆盖正常 + 失败 + 边界(如 `edit_file` 非唯一匹配、`max_iterations` 触顶、权限拒绝入 history)。
- 测试用 Mock Provider / 临时目录,不依赖真实网络或真实文件系统状态。

---

## AI 协作沉淀(.ai_history/logs/)

关键对话与决策沉淀为**决策记录**,非聊天存档。该目录 **commit 进仓库**(目的之一:向评审呈现设计主导过程)。

### 形式
- 位置:`.ai_history/logs/YYYY-MM-DD-NN-<topic>.md`,粒度见下方触发。
- 记录结构(蒸馏,非逐字稿):

  ```
  # YYYY-MM-DD · NN · <topic>
  
  ## 决策
  - <一句话决策> | 选:X | 弃:Y(理由)、Z(理由) | 主导:用户/讨论收敛 | 依据:code/tests/bench/spec
  
  ## 变更
  - <本次 code/spec 改了什么>
  
  ## 待决
  - <open question>
  
  ## 引用
  - OpenSpec change/spec id;跨越的 session log
  ```

### 禁止写入
- API key、token、任何凭据;绝对路径;与决策无关的过程噪声。

### 触发(两类,职责不重叠)
1. **Session checkpoint**(用户显式发起,于会话收尾 / 重大决策后 / context 压缩前):记跨 change 的主导判断、调试方向、open question。粒度 = 一次工作会话。
2. **OpenSpec archive**:每次 archive 必须在**同一提交内**附一条决策记录,聚焦该 change 的最终定案与被否决备选;**引用**其跨越的 session log,不复制内容。粒度 = 一个 change。

### 规则
- 两类均由 **Agent 起草、用户审阅后**随相关提交入库。
- **未被显式要求时,Agent 不得自动写入或修改本目录。**
- 与 OpenSpec 边界:成为 spec 变更的决策归 `specs/` / proposal,本 log 仅引其 id;log 的独占职责是 rationale、rejected alternatives、设计主导记录。**不两头维护。**
