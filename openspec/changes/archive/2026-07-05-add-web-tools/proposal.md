# add-web-tools

## Why

当前 agent **零联网能力** —— 7 个内置工具全是本地(`list_dir` / `read_file` / `glob` / `grep` / `write_file` / `edit_file` / `run_shell`)。想读文档 / 网页、或搜索资料,只能靠 `run_shell` 调 `curl`(clunky、还走 Execute 权限弹窗)。这挡住 research-first 工作流(也是后续 L1 plan 模式「有根调研」的前置)。对标 Claude Code / OpenCode 都内置了 web 工具。

v1 走**最省事的 T0**:`web_fetch` 客户端抓取、`web_search` 打 DuckDuckGo HTML 端点 —— **零 key、复用现有 `reqwest`、不加新依赖**。后端藏在 seam 后,以后要稳再换 Brave/Tavily(接口不变)。

## What Changes

1. **`web_fetch(url)`**(ReadOnly):HTTP GET 一个 URL(带浏览器 User-Agent + 超时)→ HTML 转可读文本(去标签、解实体、折叠空白)→ 超 `ToolContext.max_output_bytes` 截断置 `truncated`(仿 `read_file`/`grep`);非 2xx / 超时 / 非 HTML → `is_error`、不 panic。
2. **`web_search(query)`**(ReadOnly、**免 key**):打 `https://html.duckduckgo.com/html/?q=<编码 query>` → 解析前 ~8 条结果(标题 / 摘要 / URL,**DDG 结果链接是 `/l/?uddg=<编码真链>` 重定向,须解 `uddg` + percent-decode 拿真 URL**)→ 格式化文本;抓取失败 / 无结果 → `is_error`。
3. **可测性**:纯解析(`html_to_text` / `parse_ddg_results` / `decode_uddg` / `ddg_search_url`)做纯函数强制 TDD;HTTP 抓取经注入的 `WebFetcher` seam(real = reqwest,test = mock canned HTML)→ 工具 execute 全程 Mock 可测、不触网。

## Impact

- 修改 capability:`builtin-tools`(ADD:`web_fetch` + `web_search` 两条工具契约,内置工具 7 → 9;archive 时手改 Purpose「7→9 / 4→6 只读」+ `--skip-specs`);`tool-system` **仅代码**(`default_registry` 多注册 2 个,**无 spec delta**——tool-system spec 不枚举注册集)
- Affected code:`src/tool/web.rs`(新:`WebFetcher: Send+Sync` trait + `WebError` + `ReqwestFetcher` + 两工具 + 纯解析函数);`src/app.rs`(`default_registry` 注册 2 个 + 断言);`src/tool/mod.rs`(`pub mod web`)
- **无新依赖**(HTTP 复用现有 `reqwest`、URL percent 编解码用 `reqwest::Url`(re-export `url`)、HTML 解析用现有 `regex`)
- 回退:纯增两只读工具;不注册即完全无影响。已知局限(T0 stopgap):DDG 限流 / 封锁、regex 解析脆(DDG 改 markup 会失效)—— 接受,升级 = 换 `web_search` 后端、工具接口不变。**安全已知局限**:v1 不做 SSRF / 内网防护(`reqwest` 默认跟重定向,公网 URL 可 302 到内网 / 云元数据 `169.254.169.254`)—— 本地单用户 CLI 可接受,云 / CI 环境须自行防护
