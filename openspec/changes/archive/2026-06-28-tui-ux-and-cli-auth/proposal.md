## Why

1.1.x 完善 4 项用户高频痛点(均未在技术方案 §13 路线图规划):

1. 输入框输 `/` 时**无命令补全提示**,用户记不住命令;
2. TUI 捕获鼠标导致**无法用终端原生选择复制**;
3. **状态栏在输入框上方**,用户要对齐 claude code 的底部状态栏(输入框在上、状态栏在最底);
4. **无便捷的模型 / 凭据配置方式**,只能手编 `config.toml` + `credentials`。

本 change 一次补齐:TUI 交互三项(补全 / 复制 / 布局)+ CLI `auth` 子命令(opencode 式持久化配置 provider / model / key)。

## What Changes

- **点1(tui-shell + builtin-commands)**:输 `/` 弹命令补全(前缀匹配 + 描述,`↑↓` 选 / `Tab`|`Enter` 补全 / `Esc` 关);`builtin-commands` 暴露命令**元数据**(名 + 描述 + 用法)供补全列表。
- **点2(tui-shell)**:去掉 `EnableMouseCapture`,恢复终端原生选择复制 + scrollback(滚轮在 Windows Terminal 本就无效、键盘滚动已兜底);清理 `handle_scroll_mouse` 死代码。
- **点3(tui-shell)**:`render` 布局调换 —— **输入框在上、状态栏在最底**(adapt 设计规范 02;贴 claude code)。
- **点4(cli-runtime + credential-source + config-layering)**:新增 `mysteries auth` CLI 子命令,交互式(API key **隐藏输入**)配置 provider / base_url / model / key,持久化写 `config.toml` + `credentials`;`credential-source` / `config` 加**写**能力。TUI `/model` 保留(运行时临时切)。

## Capabilities

### Modified Capabilities
- `tui-shell`: **ADD**「`/` 命令补全」、**ADD**「终端原生复制(不捕获鼠标)」、**MODIFY**「ratatui 四区最小外壳渲染」(状态栏移至输入框下方、权限框移至输入框上方)。
- `builtin-commands`: **MODIFY**「slash 命令解析」(暴露命令元数据:名 / 描述 / 用法)。
- `cli-runtime`: **ADD**「`auth` 子命令交互式配置」。
- `credential-source`: **ADD**「凭据写入 FileCredentialSink」。
- `config-layering`: **ADD**「配置写入(merge 持久化)」。

## Impact

- **code**:`tui/{render,app,command,terminal,mod}`(补全 / 复制 / 布局);`cli.rs` + `main`(`auth` 子命令 + 隐藏输入);`credential/`(写);`config/`(写);`app.rs`(auth 写装配)。
- **apply 拆分(一个 change,两 agent 并行)**:
  - **TUI 组(点1/2/3)** = `src/tui/*` + `builtin-commands` 元数据;
  - **CLI 组(点4)** = `cli.rs` / `main` / `credential/` / `config/` / `app.rs`(`auth` 路径)。
  - 两组改**不同文件**、可并行 worktree apply;主 agent 合到本 change 一起 archive。
- **安全红线(点4)**:API key **隐藏输入**(不回显)+ `secrecy::SecretString` 不入日志 / 错误 + `credentials` 文件权限(Unix `0600`;Windows 尽力);写配置 **merge 保留**用户其他字段、不整文件覆盖。
- **设计偏离**:点3 状态栏移底 = **adapt** 设计规范 02(原型状态行在输入框上方;更贴 D2「底部状态行」字面 + claude code)。
- **deps**:隐藏输入用既有 `crossterm` raw mode,**零新增**。
