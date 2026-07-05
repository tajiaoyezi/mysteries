# 2026-07-05 · 52 · archive add-web-tools

## 决策
- v1 联网能力走 T0 = DuckDuckGo HTML 端点(免 key)| 选:DDG keyless + 复用 `reqwest` | 弃:Brave/Tavily(需 key,留作 seam 后端升级)、provider-native server search(耦合 provider)| 主导:用户拍板 | 依据:live 真抓验证 markup 契约仍成立
- HTML 解析 regex 手搬、零新依赖 | 选:现成 `regex` + `reqwest::Url`(percent 编解码)| 弃:`scraper`/`html2text`/`percent-encoding`(dep 节黙)| 主导:用户 | 依据:code(`git diff Cargo.toml` 空)
- 注入 `WebFetcher: Send + Sync` seam | real=`ReqwestFetcher` / test=`MockFetcher`,execute 全程离线可测 | 依据:compiler(`Tool: Send + Sync`,漏 bound → `Box<dyn Tool>` coerce E0277)
- SSRF / 内网 v1 不设防 | 已知局限,durable 记入 proposal + spec-delta | 依据:真机实证——`web_fetch` 无阻读 `127.0.0.1:8765`、对 `169.254.169.254` 亦发请求(仅目标不可达),证不过滤特殊 IP 段
- `decode_uddg` 加 `&amp;` 预处理 | 真 DDG href 属性是 `&amp;rut=`(HTML 转义)| 主导:执行 agent 真机发现 | 依据:live 真抓(fixture 与 live 首条逐字节同 `rut` hash);authority 真实 HTML > spec 里写的裸 `&rut=`
- `html_to_text` 数字实体通用解(hex + 十进制)| 依据:DDG 实发 hex `&#x27;` 非十进制 `&#39;`,硬列会漏解成 `Rust&#x27;s`

## 变更
- 新增 `src/tool/web.rs`:`web_fetch` / `web_search`(ReadOnly)+ 纯函数 `ddg_search_url`/`decode_uddg`/`html_to_text`/`parse_ddg_results` + `WebFetcher`/`ReqwestFetcher`/`WebError`
- `default_registry` 注册 2 工具(内置 7 → 9);`src/tool/fs.rs` `truncate_utf8` → `pub(crate)` 复用
- spec `builtin-tools`:ADD 2 requirement(archive 自动追加)+ Purpose 手改(7→9 / 4→6 只读——delta 表达不了 overview)
- 无新依赖(`reqwest` / `regex` / `futures-util` / `thiserror` / `async-trait` 皆已在)

## 待决
- T0 DDG 会飘(限流 / markup 变)→ 升级 = 换 `web_search` seam 后端(Brave/Tavily 返 JSON),工具名 / schema / 模型侧不变
- 可选 SSRF 廉价护栏(拒初始 URL 内网 host)留待需要
- `html_to_text` 每调重编 6 regex(可 `LazyLock` 零依赖)、标签边界粘词补空格 —— later,不进本次
- 用户文档 `README.md`(第 17/36 行「7 个内置工具」)与 `技术方案`(「1.0 的 7 个工具」)未同步——非本 change artifact,另议

## 引用
- OpenSpec change: `add-web-tools`(archived `2026-07-05-add-web-tools`)
- 跨:add-command-allowlist(log 51,fmt 基线 + toolchain pin)
