# builtin-tools Delta

## ADDED Requirements

### Requirement: web_fetch 抓取网页文本(ReadOnly)

`web_fetch` SHALL 对给定 `url` 发 HTTP GET(带浏览器 User-Agent 与超时常量 `WEB_TIMEOUT`、响应体字节封顶防 OOM),将响应正文由 HTML 转为可读文本(剥 `<script>`/`<style>`、去标签、解 HTML 实体含 hex/十进制数字实体、折叠空白);权限级别 `ReadOnly`;文本超 `ToolContext.max_output_bytes`(字节、按 UTF-8 边界)时 SHALL 截断并置 `ToolOutcome.truncated = true`;非 2xx 响应、超时、网络错误、二进制 content-type(非 `text/*`;缺 `Content-Type` 视为可读)SHALL 编码为 `ToolOutcome{is_error: true}`,不 panic。**v1 SHALL NOT 防护 SSRF / 内网地址(含跟随重定向到内网 / 云元数据)—— 已知局限**。HTTP 抓取经可注入的 `WebFetcher`(`: Send + Sync`)seam(async),测试以 mock fetcher 提供 canned 内容/错误,不依赖真实网络。HTML→文本 SHALL 抽为纯函数 `html_to_text`(headless 强制 TDD)。

#### Scenario: 抓取并转文本

- **WHEN** 注入返回一段含标签的 HTML 的 mock fetcher,对某 `url` 调用 `web_fetch`
- **THEN** `ToolOutcome.content` 为去标签/解实体后的可读文本,`is_error = false`

#### Scenario: 抓取失败编码 is_error

- **WHEN** mock fetcher 返回错误(或超时/非 2xx)
- **THEN** 返回 `is_error = true` 的 `ToolOutcome`(不 panic)

#### Scenario: 输出超限截断

- **WHEN** 转出文本超过 `ToolContext.max_output_bytes`
- **THEN** `content` 被截断,`truncated = true`

#### Scenario: html_to_text 纯函数(可单测)

- **WHEN** 对含 `<script>`、标签、HTML 实体、连续空白的 HTML 调 `html_to_text`
- **THEN** script/style 内容不保留、标签去净、实体解码、连续空白折叠;畸形/空 HTML 不 panic

### Requirement: web_search 网络搜索(ReadOnly,DuckDuckGo T0)

`web_search` SHALL 对给定 `query` 打 DuckDuckGo HTML 端点(`https://html.duckduckgo.com/html/?q=<percent-encoded query>`),解析前若干条(`MAX_SEARCH_RESULTS`)结果为 `{title, url, snippet}` 并格式化为文本;权限级别 `ReadOnly`,**免 API key**。DDG 结果链接为 `/l/?uddg=<percent-encoded 真链>` 重定向,`web_search` SHALL 解出 `uddg` 参数并 percent-decode 得**真实 URL**(供后续 `web_fetch`);抓取失败或解析出 0 条结果 SHALL 编码为 `ToolOutcome{is_error: true}`,不 panic。抓取经同一 `WebFetcher` seam(mock 可测);URL 构造 `ddg_search_url`、重定向解码 `decode_uddg`、结果解析 `parse_ddg_results` SHALL 抽为纯函数(headless 强制 TDD)。搜索后端(DDG)藏于该 seam 后,后续换外部搜索 API 不改工具接口。

#### Scenario: 解析 DDG 结果并还原真链

- **WHEN** 注入返回样例 DDG HTML 的 mock fetcher,调用 `web_search`
- **THEN** `content` 含解析出的标题/摘要与 **percent-decode 后的真实 URL**(非 DDG 重定向),`is_error = false`

#### Scenario: 无结果编码 is_error

- **WHEN** 抓取失败,或返回的 HTML 解析出 0 条结果
- **THEN** 返回 `is_error = true` 的 `ToolOutcome`(不 panic)

#### Scenario: decode_uddg / parse_ddg_results 纯函数(可单测)

- **WHEN** 对**真实形态** `//duckduckgo.com/l/?uddg=https%3A%2F%2Fa.com%2Fx&rut=<hex>` 调 `decode_uddg`(uddg 后带 `&rut=` 尾);对**真抓样例** DDG HTML 调 `parse_ddg_results`
- **THEN** 前者得 `https://a.com/x`(正确止于 `&`、不吞 `rut` 尾;非重定向 / 广告 href → `None`);后者得 `{title,url,snippet}` 列表(url 为解码后真链、非 DDG 重定向)、至多 `MAX_SEARCH_RESULTS` 条
