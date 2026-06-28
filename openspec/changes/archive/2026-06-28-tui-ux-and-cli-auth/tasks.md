# Tasks — tui-ux-and-cli-auth

> 一个 change,apply **两组并行**(改文件不相交,各自 worktree):
> - **TUI 组**(点1/2/3):`src/tui/*` + builtin-commands 元数据;
> - **CLI 组**(点4):`cli.rs` / `main` / `credential/` / `config/` / `app.rs`(auth 路径)。
> TDD:补全数据/逻辑、config 写、credential 写、auth 流程均 headless 内核,**强制红-绿**;TUI 渲染(补全浮层 / 布局 / 去捕获)走 insta 事后 + 人工对眼。
> 🔴 红灯停点:**①** 命令补全数据+逻辑(TUI 组)**②** 凭据写入 FileCredentialSink(CLI 组,安全敏感)**③** auth 交互流程(CLI 组,输入可注入)。各测试首次成型、贴运行时失败后停下等确认。

## TUI 组 —— 点1/2/3(worktree A)

### 1. `/` 命令补全(builtin-commands + tui-shell,强制 TDD + insta)
- [x] 1.1 【红】builtin-commands 加命令元数据清单(名/简述/用法),与 `parse_command` 命令集同源;测:元数据覆盖各内置命令、与 parse 识别集一致。运行确认失败。
- [x] 1.2 🔴 **红灯停点①**:贴元数据 + 补全过滤/选择逻辑测试 + 失败输出,**停下等确认**。
- [x] 1.3 【绿】补全状态(候选列表 + 高亮 index）+ 输入处理:`/` 前缀触发过滤、`↑↓` 移高亮、`Tab`/`Enter` 补全、`Esc` 关、非命令态不弹。
- [x] 1.4 insta(对眼):补全浮层带色快照(midnight）；人工对设计规范 C 系列。

### 2. 终端原生复制 —— 去鼠标捕获(tui-shell,删码 + 冒烟)
- [x] 2.1 去 `terminal.rs` 的 `EnableMouseCapture`/`DisableMouseCapture`;删 `MouseEvent` 处理 + `handle_scroll_mouse`(死代码);键盘滚动保持不变。
- [x] 2.2 零回归:既有滚动 / 按键测保持绿(滚轮相关测随死代码移除而删,记录于变更)。人工冒烟(留用户):终端可选择复制。

### 3. 状态栏移底(tui-shell,render + insta)
- [x] 3.1 `render.rs` 布局把「状态行→输入框」改为「输入框→状态行(最底)」;权限框内联钉于输入框上方。
- [x] 3.2 insta(对眼 + 迁移):欢迎态 / 权限态等受影响快照重生成 + 人工审(状态栏在最底、权限框在输入框上方）。

## CLI 组 —— 点4(worktree B)

### 4. 配置写入(config-layering,强制 TDD)
- [x] 4.1 【红】测:`write_config`(read-modify-write merge):对含 `max_iterations` + `model` 的 config.toml 写 `model="new"` → 回读 model=new 且 max_iterations 保留;文件不存在时新建。运行确认失败。
- [x] 4.2 【绿】实现 merge 写(保留其他字段、不整覆盖)。

### 5. 凭据写入(credential-source,强制 TDD)
- [x] 5.1 【红】测:`write_credential`/`FileCredentialSink` upsert:初始含 `anthropic=sk-a`,对 openai 写 sk-o、对 anthropic 写 sk-a2 → 文件含 openai=sk-o + anthropic=sk-a2(替换非重复)、其他行保留;明文不入返回错误。运行确认失败。
- [x] 5.2 🔴 **红灯停点②**:贴 5.1 测试 + 失败输出,**停下等确认**(凭据写入、安全敏感)。
- [x] 5.3 【绿】upsert 实现(SecretString 取明文集中、Unix 0600 权限）；路径注入可测。

### 6. auth 子命令(cli-runtime,强制 TDD + 隐藏输入)
- [x] 6.1 【红】测:auth 流程**输入读取可注入**(类 StdinDecider),注入 provider=openai/model=gpt-4o/key=sk-xxx → 调 write_config + write_credential(临时目录)产出正确 config.toml + credentials;EOF/取消 → 不写;全程不触网。运行确认失败。
- [x] 6.2 🔴 **红灯停点③**:贴 6.1 测试 + 失败输出,**停下等确认**(auth 流程)。
- [x] 6.3 【绿】`mysteries auth` 流程(provider/base_url/model/key 依次读,输入读取解耦可注入）；`main` 分流加 `auth`。
- [x] 6.4 API key 隐藏输入:既有 `crossterm` raw mode 不回显读取、读毕恢复;key 经 SecretString,提示/错误不含明文。(终端交互人工冒烟)

## 收尾(主 agent 合两组后)
- [x] 7.1 `cargo build`/`cargo test`(全 target)全绿。
- [x] 7.2 `openspec validate tui-ux-and-cli-auth --strict` 通过。
- [x] 7.3 `cargo clippy --all-targets -- -D warnings` 零警告;`cargo fmt --check` 净。
- [ ] 7.4 手动冒烟(留用户):`/` 补全;终端选择复制;状态栏在最底;`mysteries auth` 配置后进 TUI 可用。
