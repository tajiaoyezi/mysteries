# Design — add-web-tools

> 经两路对抗审查(含真抓 live DDG)修订:`WebFetcher` Send+Sync 编译阻断、`ToolContext` 无 timeout 字段、`decode_uddg` 真 href 带 `&rut=` 尾、DDG 发 hex `&#x27;`、percent 编解码走 `reqwest::Url`、SSRF 已知局限、字节封顶——详见各决策注。

## 决策

### D1 两工具皆 ReadOnly
- `web_fetch` / `web_search` 无副作用(只读网络)→ `permission_level = ReadOnly` → 门直接放行、不弹窗。与本地只读工具(`read_file`/`grep`)同待遇。

### D2 复用 reqwest,零新依赖;超时用常量、响应字节封顶
- `reqwest` 0.12 已依赖(transport 用它打 LLM)→ 复用。`ReqwestFetcher` 持一个 `reqwest::Client`,GET 带**浏览器 User-Agent**(**审查真抓证:带 UA 时 DDG 返 200/10 结果**,load-bearing)+ **超时常量 `WEB_TIMEOUT = Duration::from_secs(15)`**(**审查:`ToolContext` 只有 `{cwd, max_output_bytes}`、无 timeout 字段——不 plumb ToolContext、不加字段**)。
- **响应体先按字节封顶**(检 `content-length` 或 cap `.bytes()`,如 `WEB_MAX_BYTES = 2 MiB`)再转文本——防巨页 `.text()` 先 OOM 再截断(审查 M6)。
- HTML 解析用现有 `regex`;**percent 编解码走 `reqwest::Url`**(见 D3)。**不引入** scraper / html2text / percent-encoding / 搜索 SDK(用户拍板 regex 手搬 + dep-节黙)。

### D3 纯解析 + 注入抓取(强制 TDD)
- **`WebFetcher: Send + Sync`**(**审查 COMPILE BLOCKER**:`Tool: Send + Sync`(tool/mod.rs:12);漏 bound 则 `Box<dyn WebFetcher>` 为 `!Send`、工具无法 coerce 成 `Box<dyn Tool>`、E0277;`CredentialSource`/`PermissionDecider`/`Provider` 均声明此 bound):
  ```
  #[async_trait] trait WebFetcher: Send + Sync { async fn fetch(&self, url: &str) -> Result<String, WebError>; }
  ```
  real = `ReqwestFetcher`,test = `MockFetcher`(canned HTML / 预置 `Err`);工具持 `Box<dyn WebFetcher>`。`WebError` = 携**可显示原因**的错误类型(D6 用其填 is_error content)。
- **纯函数(强制 TDD);percent 编解码经 `reqwest::Url`**(reqwest re-export `url` 2.5、**免新依赖、lossy 不 panic**):
  - `ddg_search_url(query) -> String` = `reqwest::Url::parse_with_params("https://html.duckduckgo.com/html/", &[("q", query)])` → `.to_string()`(自动编码。**注:空格编为 `+`(form-encoding),DDG 接受;测试断言 impl 实际产出的形式**)。
  - `decode_uddg(href) -> Option<String>`:**经 `reqwest::Url` 取 `uddg` query pair**(自动按 `&` 切分 + percent-decode)。**审查(live 证):真 href = `//duckduckgo.com/l/?uddg=<percent-enc 真链>&rut=<64hex>`——`uddg` 后接 `&rut=`,贪婪 `uddg=(.+)"` 会吞尾成垃圾 URL**;`query_pairs()` 正确止于 `&`。协议相对 `//…` → parse 前补 `https:`。无 `uddg`(如广告 `y.js` href)→ `None`,调用方回退原 href。
  - `html_to_text(html) -> String`:剥 `<script>…</script>`/`<style>…</style>` 整块 → 去 `<[^>]*>` → 解实体:命名 `&amp; &lt; &gt; &quot; &nbsp;` + **数字实体通用解**(**hex `&#x[0-9a-fA-F]+;` + 十进制 `&#[0-9]+;`——审查 live 证:DDG 实发 hex `&#x27;`、非十进制 `&#39;`;硬列 `&#39;` 会漏解成 `Rust&#x27;s`**)→ 折叠连续空白 + trim;畸形/空不 panic。
  - `parse_ddg_results(html) -> Vec<SearchResult{title,url,snippet}>`:regex 抠 `class="result__a" … href="H"`(attr 顺序 `rel…class…href`)与 `class="result__snippet"`;**title 与 snippet 均过 `html_to_text`**(审查:title 也可含 `<b>` 高亮);url = `decode_uddg(H).unwrap_or(H)`;取前 `MAX_SEARCH_RESULTS = 8`。**样例 HTML fixture 必须用真抓 DDG 片段**(带 `&rut=` 尾 + `&#x27;` + `<b>` + 真 `result__a/__snippet` 结构),**不得手搓**(审查:手搓 = 测试照实现反推、测不出真机漂,违 TDD 纪律)。
- **注入 seam**(仿 Provider/PermissionDecider/CredentialSource):MockFetcher 喂 canned HTML/Err → execute 全程离线可测、不触网。

### D4 web_search 后端 = DDG(T0),藏 seam 后可换
- v1 后端 = DuckDuckGo HTML 端点(`https://html.duckduckgo.com/html/?q=`,免 key)。**真抓验证**(browser UA + GET):HTTP 200、`text/html; charset=UTF-8`、~35KB、10 条真结果、无 CAPTCHA/无-JS 页;`result__a`/`result__snippet` 现行有效。**T0 脆性**:限流 / 偶发封锁页(有效 HTML 但 0 结果 → `is_error`,error 文案宜注明「无结果/疑似限流」免模型误判)、regex 依赖 DDG markup。升级 = 换 seam 打的目标 + `parse_*`(Brave/Tavily 返 JSON),**工具名 / schema / 模型侧 / loop 全不动**。

### D5 输出与截断
- `web_fetch`:fetcher 层字节封顶(D2)→ `html_to_text` → 文本超 `ToolContext.max_output_bytes`(UTF-8 边界)截断 + `truncated = true`(**复用 `read_file`/`grep` 的 `truncate_utf8` fs.rs:327**,不引新常量)。
- `web_search`:前 `MAX_SEARCH_RESULTS = 8`;格式化「N. 标题 — URL\n  摘要」文本块。

### D6 错误一律编码 is_error(不 panic)
- 非 2xx、超时、网络错、DDG 解析 0 条 → `ToolOutcome{is_error: true}`,content 带简短原因(仿既有工具立场)。
- **content-type**:接受 `text/*`(HTML + 纯文本/markdown;`html_to_text` 对纯文本近乎 no-op)+ **缺 `Content-Type` 视为可读**——**审查:只收 `text/html` 会误拒 `text/plain`/`application/json`/`.txt` 等可读目标**;明确二进制(`image/*`/`application/octet-stream`/PDF 等)→ is_error。

## 接缝(实现挂载点)
- `src/tool/web.rs`(新):`WebFetcher: Send+Sync` trait + `WebError` + `ReqwestFetcher`(UA / `WEB_TIMEOUT` / 字节封顶 / content-type 门);`WebFetchTool` / `WebSearchTool`(`impl Tool`、`Box<dyn WebFetcher>`);纯函数 `ddg_search_url`/`decode_uddg`/`html_to_text`/`parse_ddg_results` + `SearchResult`;percent 编解码经 `reqwest::Url`。
- `src/tool/mod.rs`:`pub mod web;`。
- `src/app.rs` `default_registry`:注册两工具(`ReqwestFetcher`);更新 `default_registry_contains_all_builtin_tools`(加 2 名)。
- 工具描述:`web_search` =「联网搜索,返回标题/摘要/URL;拿 URL 用 web_fetch 深读」;`web_fetch` =「抓 URL 返可读文本,读文档/网页」。

## 风险 / 权衡
- **DDG T0 会飘**(限流/封锁)—— stopgap,真机验能用即可,飘了换后端。
- **regex 解析 HTML 脆** —— DDG 改 markup 会失效;fixture 用真抓片段挡一部分;`html_to_text` 对畸形 HTML 尽力而为(够喂模型即可)。
- **SSRF / 内网(已知局限,durable 记入 proposal + spec-delta)**:`reqwest::Client` 默认**跟重定向** → 公网 URL 可 302 → `http://169.254.169.254/…`(云元数据)/ `http://localhost:…`;dev 机 blast radius = 自身 localhost,云/CI 上是**凭据泄露**。v1 不做 SSRF 防护;**可选廉价护栏**:拒**初始 URL** 的 loopback/RFC1918/link-local host(不拦重定向则仍有放大面,须注明)。
- `.text()` charset-off → 非 UTF-8 页 mojibake(接受,「够喂模型」)。

## 定案(细点)
1. `MAX_SEARCH_RESULTS = 8`;web_fetch 复用 `max_output_bytes` 截断 + fetcher 层 `WEB_MAX_BYTES ~2 MiB` 封顶。
2. 超时 = `WEB_TIMEOUT = 15s` 常量(非 `ToolContext`)。
3. percent 编解码 = `reqwest::Url`(`parse_with_params` / `query_pairs`)。
4. SSRF v1 不做(已知局限,durable);可选加初始 URL 内网拒绝。
