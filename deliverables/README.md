# Deliverables —— 可运行的验证产物

本项目暂不发布到 crates.io。本目录提供**无需从源码编译**即可验证「mysteries 真能跑」的产物。

## 📦 预编译二进制

- **`mysteries-v1.1.0-windows-x64.exe`** —— Windows x64 的 release 构建。
  ```powershell
  ./mysteries-v1.1.0-windows-x64.exe auth login   # 配置 provider + API Key
  ./mysteries-v1.1.0-windows-x64.exe              # 进入 TUI
  ```
  其它平台请自行 `cargo build --release`(产物在 `target/release/`)。

## 🐍 Snake-Rogue demo

- **`Snake-Game/`** —— 由 mysteries **亲手生成**的贪吃蛇 + 肉鸽单文件 HTML 游戏,浏览器打开 `index.html` 即玩;证明 agent 能从需求出发、经 plan 审批、逐步交付一个真正可玩的软件。详见 [`Snake-Game/README.md`](Snake-Game/README.md)。

## 📸 界面截图

- **`mysteries截图/`** —— TUI 各态截图:欢迎、`/` 命令补全、`/models` 热切换、权限确认(diff + y/n)、Plan 全流程(思考 / ask_user / 计划审批 / 进度面板)、markdown 代码高亮、CLI `auth login` 等。
