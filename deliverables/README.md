# Deliverables —— 演示与历史验证产物

本项目暂不发布到 crates.io。当前正式预编译包通过 [GitHub Releases](https://github.com/tajiaoyezi/mysteries/releases) 交付，并附带 `SHA256SUMS`；本目录只保存演示内容、截图与历史验证产物，不是当前 binary 分发渠道。

## 📦 历史验证二进制

- **`mysteries-v1.1.0-windows-x64.exe`** —— v1.1.0 阶段提交的 Windows x64 历史验证构建；该里程碑没有 Git tag / GitHub Release，不应作为当前安装源。
  ```powershell
  ./mysteries-v1.1.0-windows-x64.exe auth login   # 配置 provider + API Key
  ./mysteries-v1.1.0-windows-x64.exe              # 进入 TUI
  ```
  当前版本请从 GitHub Release 下载 Windows/Linux archive，或自行 `cargo build --release`（产物在 `target/release/`）。v1.2.0 及后续 binary/archive/checksum 不再提交到本目录。

## 🐍 Snake-Rogue demo

- **`Snake-Game/`** —— 由 mysteries **亲手生成**的贪吃蛇 + 肉鸽单文件 HTML 游戏,浏览器打开 `index.html` 即玩;证明 agent 能从需求出发、经 plan 审批、逐步交付一个真正可玩的软件。详见 [`Snake-Game/README.md`](Snake-Game/README.md)。

## 📸 界面截图

- **`mysteries截图/`** —— TUI 各态截图:欢迎、`/` 命令补全、`/models` 热切换、权限确认(diff + y/n)、Plan 全流程(思考 / ask_user / 计划审批 / 进度面板)、markdown 代码高亮、CLI `auth login` 等。
