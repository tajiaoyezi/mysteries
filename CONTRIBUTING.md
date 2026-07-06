# 贡献指南

欢迎参与 mysteries。本项目对**工程流程**有明确约定(这也是它区别于一般玩具项目的地方),请在动手前读完本页。

## 环境准备

- Rust **stable**(仓库根 `rust-toolchain.toml` 已钉版本 + `rustfmt`/`clippy` 组件,`rustup` 会自动装)。
- 无需其它系统依赖(TLS 走 `rustls`、语法高亮走纯 Rust 的 `syntect` regex-fancy 引擎、无 C `onig`/`openssl`)。

```bash
cargo build              # 构建
cargo run                # 进 TUI(首次会引导 auth login)
cargo run -- --headless "解释一下 src/agent 的结构"   # 无头单轮
```

## 质量基线(提交前必过)

```bash
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test                 # ⚠️ 跑全量,别用 --lib —— --lib 不含 tests/ 集成测试
```

CI(`.github/workflows/ci.yml`)在 Windows + Linux 上强制这四关。**`cargo test` 必须跑全量**:曾有一条集成断言因门禁只跑 `--lib` 潜伏一周未被发现。

## 工程流程:OpenSpec 先行

不直接写大段代码。每个变更走 **propose → apply → archive** 三步:

1. **propose**:`openspec new change <name>`,写 `proposal.md`(为什么/改什么)、`design.md`(怎么改 + 取舍)、`tasks.md`(实现步骤)、`specs/<能力域>/spec.md`(RFC 2119 风格的 spec delta)。`openspec validate <name> --strict` 通过。
2. **apply**:按 `tasks.md` 实现,逐条勾选。
3. **archive**:`openspec archive <name> -y`(自动把 delta 合进主 `openspec/specs/`),**同一提交内**附一条决策记录到 `.ai_history/logs/`(记最终定案 + 被否决备选 + 依据)。

## TDD(测试驱动)

- **内核强制 TDD**(Agent Loop / 工具系统 / 权限门 / Provider 归一化 / 配置 merge 等纯逻辑):先写失败测试 → 确认因断言失败而红(非编译错)→ 写最小实现转绿。
- **TUI 外壳**(ratatui 渲染/布局/交互):用 `TestBackend` + `insta` 快照做**事后**回归,不走 red-green。
- 对存量代码补测属 characterization(非 TDD):期望值从 spec 推、别照实现反抄,并做 mutation 抽检(改坏被测行确认测试变红)防"假绿"。

## 提交规范

- 首行用 [Conventional Commits](https://www.conventionalcommits.org/) 英文前缀(`feat:`/`fix:`/`refactor:`/`test:`/`docs:`/`chore:`),其后描述可中文。
- 对话/注释/文档用简体中文;技术词(标识符、命令、库名、协议名、报错)保留英文原文。
- 破坏性操作(文件覆盖删除、`git` 写、`shell` 执行)先说明再动手——与产品自身的权限模型一致。

## 权威次序

冲突时以 **code / 编译器 / 测试 > spec > 推断** 为准,并显式指出冲突,不静默选边。
