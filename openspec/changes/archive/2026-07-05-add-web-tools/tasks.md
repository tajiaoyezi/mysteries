# Tasks — add-web-tools

红灯纪律:红灯独立成步,以断言失败落红(非编译错)——新类型/新签名允许红灯内先落桩(**桩须返回一个"错误的" `ToolOutcome` 令断言翻红,非 `todo!()`/panic**)。**红灯停点**:2.1 为**新 trait(`WebFetcher`)+ 新工具(`WebFetchTool`/`WebSearchTool`)接口**首次成型,测试 + 失败输出贴出后**停下等确认**;1.x 纯解析函数可连写。
执行 agent MUST NOT:git 写操作、修改既有快照/夹具以过测(**例外见 3.1**:`default_registry` 断言加 2 个工具名是合法行为变更)、勾选第 5 节真机任务、**全仓 `cargo fmt`**(只碰你改的)、kill 用户进程、**加新依赖**(HTTP 用 `reqwest`、URL 编解码用 `reqwest::Url`、解析用 `regex`;`percent-encoding`/`form_urlencoded`/`scraper` 等**均不得加入 `Cargo.toml`**)。

## 1. 纯解析函数(强制 TDD;percent 编解码经 `reqwest::Url`)

- [x] 1.1 红→绿:`ddg_search_url(query) -> String` = `reqwest::Url::parse_with_params("https://html.duckduckgo.com/html/", &[("q", query)])` → `.to_string()`;`decode_uddg(href) -> Option<String>` = 经 `reqwest::Url` 取 `uddg` query pair(协议相对 `//…` → parse 前补 `https:`;无 `uddg` → `None`)。测试:query 含空格(**断言 impl 实际编码形式**,`parse_with_params` 空格为 `+`)/中文/`&`;**真实形态 href** `//duckduckgo.com/l/?uddg=https%3A%2F%2Fdoc.rust-lang.org%2Fx.html&rut=<hex>` → `https://doc.rust-lang.org/x.html`(**href 必须带 `&rut=` 尾**,验证不吞尾);广告 `//duckduckgo.com/y.js?…` 或无 `uddg` → `None`。
- [x] 1.2 红→绿:`html_to_text(html) -> String` —— 剥 `<script>…</script>`/`<style>…</style>` 整块 → 去 `<[^>]*>` → 解命名实体(`&amp; &lt; &gt; &quot; &nbsp;`)+ **数字实体通用解**(hex `&#x[0-9a-fA-F]+;` + 十进制 `&#[0-9]+;`)→ 折叠空白 + trim。测试:含 `<script>`、`<b>`、**`&#x27;`(DDG 实发的 hex 撇号)**、`&nbsp;`、多空白、畸形/空 → script/style 内容不留、标签去净、**`&#x27;` → `'`**、`&nbsp;` → 空格、空白折叠、不 panic。
- [x] 1.3 红→绿:`parse_ddg_results(html) -> Vec<SearchResult{title,url,snippet}>` —— regex 抠 `class="result__a"…href="H"`(title 内文本)与 `class="result__snippet"`;**title 与 snippet 均过 `html_to_text`**;url = `decode_uddg(H).unwrap_or(H)`;取前 `MAX_SEARCH_RESULTS = 8`。**样例 HTML 用真抓 DDG 片段**(带 `&rut=` 尾 / `&#x27;` / `<b>` / 真 `result__a`·`result__snippet` 结构),**不得手搓**。测试:解出 title / **真 url(已去 `&rut`、非 DDG 重定向)** / snippet;无结果 HTML → 空 `Vec`。

## 2. WebFetcher + 两工具(强制 TDD)

- [x] 2.1 红(**停点**):`#[async_trait] trait WebFetcher: Send + Sync { async fn fetch(&self, url: &str) -> Result<String, WebError>; }`(**`Send + Sync` 必带**——`Tool: Send + Sync`,漏则工具 coerce 不成 `Box<dyn Tool>`);`WebError`(携可显示原因);`WebFetchTool` / `WebSearchTool`(各 `impl Tool`、持 `Box<dyn WebFetcher>`、`permission_level = ReadOnly`、schema `{url}` / `{query}`、`execute` 桩**返回一个错误的 `ToolOutcome` 令断言红**);test 用 `MockFetcher`(canned HTML / 预置 `Err`)。测试(断言红):`web_fetch` 正常 HTML → `content` = `html_to_text` 结果 + `is_error=false`;`web_fetch` fetcher `Err` → `is_error`;`web_fetch` 超 `max_output_bytes` → `truncated`;`web_search` 正常 → `content` 含解析结果 + `is_error=false`;**`web_search` fetcher `Err` → `is_error`**;`web_search` 0 结果 → `is_error`。**贴测试 + 失败输出,停下等确认。**
- [x] 2.2 绿:两工具 execute 最小实现 —— `web_fetch`:`fetcher.fetch(url)` →(`Err` → is_error)→ `html_to_text` → 按 `ToolContext.max_output_bytes` 截断置 `truncated` → `ToolOutcome`;`web_search`:`ddg_search_url` → `fetch` → `parse_ddg_results` →(空 → is_error)→ 格式化 → `ToolOutcome`。`ReqwestFetcher`:reqwest client + **浏览器 User-Agent** + **`WEB_TIMEOUT = 15s`** + **响应字节封顶 `WEB_MAX_BYTES ~2 MiB`** + **content-type 门**(`text/*` / 缺失 → 放行,二进制 → `Err`);非 2xx / 网络错 → `Err`。

## 3. 注册接入

- [x] 3.1 `src/tool/mod.rs` 加 `pub mod web;`;`src/app.rs` `default_registry` 注册 `WebFetchTool` / `WebSearchTool`(`ReqwestFetcher`);更新 `default_registry_contains_all_builtin_tools` 断言(加 `web_fetch`/`web_search` 两名,**合法行为变更**)。工具描述:`web_search`=「联网搜索,返回标题/摘要/URL;拿 URL 用 web_fetch 深读」,`web_fetch`=「抓 URL 返可读文本,读文档/网页」。

## 4. 门禁

- [x] 4.1 `cargo test --lib` 全绿;`cargo clippy --all-targets -- -D warnings` 零警告;`cargo build`(exe 被占报 os error 5 即报告、别 kill;可用隔离 `CARGO_TARGET_DIR`)
- [x] 4.2 `openspec validate add-web-tools --strict` 通过;`git diff Cargo.toml` **确认无新依赖**
- [x] 4.3 **archive 时(主 agent / 用户,执行 agent 勿动)**:手改 `openspec/specs/builtin-tools/spec.md` 的 Purpose 行(「7 个内置工具」→「9」、「4 个只读」→「6」)+ `openspec archive --skip-specs`——delta 改不了 Purpose overview(见 openspec-spec-term-alignment 记忆)

## 5. 真机核验(主 agent / 用户;执行 agent MUST NOT 勾)

- [x] 5.1 `web_fetch`:让 agent 抓一个已知文档页 → 返回可读正文、无标签/实体乱码(`'`、空格正常)、超长截断
- [x] 5.2 `web_search`:让 agent 搜一个话题 → 返回若干条真实标题/URL;URL 是**真链**(非 `duckduckgo.com/l/?uddg` 重定向、无 `&rut` 尾)、能被 `web_fetch` 接着读
- [x] 5.3 组合:让 agent「查 X 并总结」→ 自主 `web_search` → 挑结果 `web_fetch` → 汇总(验证 loop 自主编排两工具)
- [x] 5.4 飘测 / 安全:DDG 限流/返空 → 工具 `is_error` 而非 panic、agent 能继续;抓一个内网 URL(如 `http://localhost:...`)观察行为(v1 已知不设防、记录实际表现)
