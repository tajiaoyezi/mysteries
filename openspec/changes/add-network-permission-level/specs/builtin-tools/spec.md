## RENAMED Requirements

- FROM: `### Requirement: web_fetch 抓取网页文本(ReadOnly)`
- TO: `### Requirement: web_fetch 抓取网页文本(Network)`
- FROM: `### Requirement: web_search 网络搜索(ReadOnly,DuckDuckGo T0)`
- TO: `### Requirement: web_search 网络搜索(Network,DuckDuckGo T0)`

## MODIFIED Requirements

### Requirement: web_fetch 抓取网页文本(Network)

`web_fetch` SHALL 对给定 `url` 发 HTTP GET(带浏览器 User-Agent 与超时常量 `WEB_TIMEOUT`、响应体字节封顶防 OOM),将响应正文由 HTML 转为可读文本(剥 `<script>` / `<style>`、去标签、解 HTML 实体含 hex / 十进制数字实体、折叠空白);权限级别 MUST 为 `Network`;文本超 `ToolContext.max_output_bytes`(字节、按 UTF-8 字符边界截断)时 SHALL 截断并置 `ToolOutcome.truncated = true`;非 2xx 响应、超时、网络错误、二进制 content-type(非 `text/*`;缺 `Content-Type` 视为可读)SHALL 编码为 `ToolOutcome{is_error: true}`,不 panic。

**Network 授权边界**:`web_fetch` MUST override tool-owned preview。preview 与 execute MUST 共用同一个纯 request builder；builder 解析出的 canonical `reqwest::Url` 同时用于 preview target 与实际传给 `WebFetcher` 的 URL。preview scope MUST 读取当前 tool 所持 `WebFetcher::permission_scope()`，formatter 不得硬编码。缺失 / 非 string / 不可解析 URL 产生 `authorizable=false`，在任何 mode 下不得 execute / fetch。一次 AllowOnce 只覆盖当前 ToolCall 按该 fetcher scope 声明的请求；下一调用仍重新授权。有效 preview 在 Yolo 可省 UI，但仍须通过 fetcher 声明并实施的 SSRF 护栏。

**SSRF 护栏**:`web_fetch` SHALL 对**初始 URL 及每一个重定向目标(逐跳)**施加同一道门:拒绝 scheme 非 `http` / `https`;host **从已解析的 URL 取**(数字编码 IP 于 parse 时已归一化;**不得从原始 URL 字符串手工切 host**,否则归一化丢失、编码绕过复活),IP 字面量直接判定、主机名经 DNS 解析后逐 IP 判定;凡落 loopback(`127.0.0.0/8`、`::1`)/ 私网(`10/8`、`172.16/12`、`192.168/16`)/ link-local(`169.254/16` 含云元数据 `169.254.169.254`、`fe80::/10`)/ unique-local(`fc00::/7`)/ CGNAT(`100.64/10`)/ NAT64(`64:ff9b::/96`)/ multicast / `0.0.0.0/8` / `240.0.0.0/4` 范围者 SHALL 拒绝(编码 `is_error`、**不发该请求**、不 panic);**DNS 解析失败或空结果 SHALL 亦拒绝(fail-closed)**。重定向 SHALL **不自动跟随**(`redirect::Policy::none`),由 `web_fetch` 手动逐跳过门、深度上限 3。IP 范围判定 `is_blocked_ip`、URL 前置检查 `precheck_url`、已解析裁决 `check_resolved`(空集=拒)SHALL 抽为纯函数(headless 强制 TDD)。**已知残留**:对抗性 DNS rebinding(护栏解析与连接层独立再解析,攻击者以 fast-TTL 在两次解析间翻转记录)——注:`check_resolved` 检查 `to_socket_addrs` 返回的**全部**地址、任一内网即拒,故稳定的多记录混合域已被拦、非残留;残留仅限主动 rebinding。升级 = 自定义 `dns::Resolve` pin 解析(使「检查即连接」);另 v6 内嵌 v4 纵深(6to4 / Teredo 等)本次未拦。T0 可接受。

HTTP 抓取经可注入的 `WebFetcher`(`: Send + Sync`)seam(async)；trait MUST 提供纯 `permission_scope()`，且任何生产实现的 fetch 行为 MUST 遵守自己声明的 `max_redirects / may_cross_origin / ssrf_each_hop`。默认生产 registry MUST 只装配 `ReqwestFetcher`，其 scope 由同一 `MAX_REDIRECTS=3` 与逐跳 SSRF 实现生成；新增生产 fetcher 必须通过 scope conformance tests。MockFetcher 仅为零网络 test double,提供 canned 内容 / 错误。HTML→文本 SHALL 抽为纯函数 `html_to_text`。

#### Scenario: web_fetch 声明 Network

- **WHEN** 查询 `web_fetch.permission_level()`
- **THEN** 返回 `PermissionLevel::Network`

#### Scenario: web_fetch preview 与实际 fetch 共用 canonical URL

- **WHEN** 对含 userinfo、IDN/punycode、数字 IP、IPv6 或默认端口的合法 URL 先取 preview、再授权执行并注入 capture WebFetcher
- **THEN** preview 的 canonical initial target 与 WebFetcher 实收 URL 来自同一 request builder、逐字节相等；permission / UI 层不重解析 host

#### Scenario: web_fetch 畸形 URL 不可授权

- **WHEN** args 缺 url、url 非 string 或 URL 无法由共享 builder 解析
- **THEN** preview 为 `authorizable=false`;即使 Yolo / decider Allow,gate 最终 Deny且 WebFetcher 零调用

#### Scenario: web_fetch preview scope 与当前 fetcher 同源

- **WHEN** WebFetchTool 持一个声明 scope 的 capture WebFetcher,读取 preview 后执行
- **THEN** preview scope 等于该 fetcher 的 `permission_scope()`；对生产 ReqwestFetcher,scope 由同一 `MAX_REDIRECTS` 及 cross-origin / 逐跳 SSRF policy 生成

#### Scenario: 生产 registry 只装配策略一致的 ReqwestFetcher

- **WHEN** 构建默认 builtin registry
- **THEN** web_fetch / web_search 均持 ReqwestFetcher；任意其他生产 WebFetcher 未通过 scope conformance tests不得装配；MockFetcher 仅在测试使用且零网络

#### Scenario: 拒绝时 WebFetcher 零调用

- **WHEN** agent gate 对一个 `web_fetch` ToolCall 返回 Deny,并注入 counting WebFetcher
- **THEN** counting WebFetcher 调用数为 0,ToolResult 为 is_error,没有 DNS / HTTP 活动

#### Scenario: 抓取并转文本

- **WHEN** 授权后注入返回一段含标签的 HTML 的 mock fetcher,对某 `url` 调用 `web_fetch`
- **THEN** `ToolOutcome.content` 为去标签 / 解实体后的可读文本,`is_error = false`

#### Scenario: 一次授权覆盖本次 redirect 链

- **WHEN** 用户 AllowOnce 一个持生产 ReqwestFetcher 的 web_fetch ToolCall,其初始公网 URL 在同一调用内发生同 origin 或跨 origin 的公网 redirect
- **THEN** 本 ToolCall 不再次发权限询问,最多跟随 3 次 redirect；下一独立 ToolCall 必须重新授权

#### Scenario: redirect budget 纯函数锁定第四跳拒绝

- **WHEN** 对纯 `redirect_allowed(redirects_followed)` 分别传 0、1、2、3，`MAX_REDIRECTS=3`
- **THEN** 0/1/2 允许处理下一次 redirect，3 拒绝第四次跟随并由 transport 返回 `too many redirects`；测试不发真实网络

#### Scenario: 抓取失败编码 is_error

- **WHEN** mock fetcher 返回错误(或超时 / 非 2xx)
- **THEN** 返回 `is_error = true` 的 `ToolOutcome`(不 panic)

#### Scenario: 输出超限截断

- **WHEN** 转出文本超过 `ToolContext.max_output_bytes`
- **THEN** `content` 被截断,`truncated = true`

#### Scenario: html_to_text 纯函数(可单测)

- **WHEN** 对含 `<script>`、标签、HTML 实体、连续空白的 HTML 调 `html_to_text`
- **THEN** script / style 内容不保留、标签去净、实体解码、连续空白折叠;畸形 / 空 HTML 不 panic

#### Scenario: 拒绝内网/loopback/link-local 目标(SSRF 护栏)

- **WHEN** 已获 Network 授权或处于 Yolo,对 `http://127.0.0.1:8765/`、`http://169.254.169.254/…`、数字编码 `http://2130706433/`、或 DNS 解析到私网 IP 的主机名调用 `web_fetch`
- **THEN** 返回 `is_error = true`(内容注明 blocked)、**不发出该请求**、不 panic

#### Scenario: 重定向到内网被拦(逐跳同门)

- **WHEN** 已获 Network 授权的 `web_fetch` 首先请求公网 URL,其响应 `302` 到内网目标(`Location` 为内网 IP 字面量、或解析到内网的主机名)
- **THEN** 首个已授权公网请求不可回滚,但目标跳过同一道 SSRF 门被拒 → `is_error = true`、**不跟随连接到内网**、不 panic

#### Scenario: is_blocked_ip 纯函数(可单测)

- **WHEN** 对 loopback / 私网 / link-local / unique-local / CGNAT / NAT64(`64:ff9b::/96`)/ multicast / `0.0.0.0/8` / `240/4` 各范围(v4 / v6 / v4-mapped)与公网 IP 调 `is_blocked_ip`
- **THEN** 前者返回 `true`、公网返回 `false`

### Requirement: web_search 网络搜索(Network,DuckDuckGo T0)

`web_search` SHALL 对给定 `query` 打 DuckDuckGo HTML 端点(`https://html.duckduckgo.com/html/?q=<percent-encoded query>`),解析前若干条(`MAX_SEARCH_RESULTS`)结果为 `{title, url, snippet}` 并格式化为文本;权限级别 MUST 为 `Network`,**免 API key**。它 MUST override tool-owned preview，且 preview 与 execute MUST 调同一个 `ddg_search_url(query)`；preview canonical target 与实际传给当前 WebFetcher 的 URL 必须相等，完整原始 query 可检查。scope MUST 读取该 fetcher 的 `permission_scope()`。缺失 / 非 string query 产生 `authorizable=false` 并在任何 mode 下零 DDG 调用。一次 AllowOnce 覆盖当前 fetcher 所声明 scope 内的 DDG 请求；下一 ToolCall 仍重新授权。生产 ReqwestFetcher 为最多 3 次、可能跨 origin、每跳 SSRF。DDG 结果链接 SHALL 解出 `uddg` 真链；结果 URL 只作文本，不额外 fetch / DNS。抓取失败或 0 结果编码 is_error。`ddg_search_url`、`decode_uddg`、`parse_ddg_results` 为纯函数。

#### Scenario: web_search 声明 Network

- **WHEN** 查询 `web_search.permission_level()`
- **THEN** 返回 `PermissionLevel::Network`

#### Scenario: web_search preview target 等于实际 DDG URL

- **WHEN** query 含空格、中文与 `&`,先取 preview，再授权执行并注入 capture WebFetcher
- **THEN** preview 保留完整原始 query；canonical target 等于 `ddg_search_url(query)` 且与 WebFetcher 实收 URL 逐字节一致

#### Scenario: web_search 畸形参数不可授权

- **WHEN** args 缺 query 或 query 非 string
- **THEN** preview 为 `authorizable=false`;即使 Yolo / decider Allow,gate 最终 Deny且 DDG WebFetcher 零调用

#### Scenario: 拒绝搜索时 WebFetcher 零调用

- **WHEN** agent gate 对一个 web_search ToolCall 返回 Deny,并注入 counting WebFetcher
- **THEN** counting WebFetcher 调用数为 0,query 未发送到 DDG,ToolResult 为 is_error

#### Scenario: web_search 授权覆盖 DDG 请求自身的 redirect

- **WHEN** 用户允许一个持生产 ReqwestFetcher 的 web_search ToolCall,DDG 初始请求在既有上限内 redirect 到另一个公网 origin
- **THEN** redirect 仍属于该次 call-scoped 授权且每跳先过 SSRF；C6 已明示最多 3 次、可能跨站；相同 query 的下一 ToolCall 仍重新询问

#### Scenario: 解析 DDG 结果并还原真链

- **WHEN** 授权后注入返回样例 DDG HTML 的 mock fetcher,调用 `web_search`
- **THEN** `content` 含解析出的标题 / 摘要与 **percent-decode 后的真实 URL**(非 DDG 重定向),`is_error = false`

#### Scenario: 搜索结果 URL 不自动抓取

- **WHEN** DDG HTML 中含多个解码后的公网结果 URL
- **THEN** WebFetcher 只被调用于本次 DDG 搜索请求,结果 URL 仅进入 content,不产生额外 fetch / DNS

#### Scenario: 无结果编码 is_error

- **WHEN** 抓取失败,或返回的 HTML 解析出 0 条结果
- **THEN** 返回 `is_error = true` 的 `ToolOutcome`(不 panic)

#### Scenario: decode_uddg / parse_ddg_results 纯函数(可单测)

- **WHEN** 对**真实形态** `//duckduckgo.com/l/?uddg=https%3A%2F%2Fa.com%2Fx&rut=<hex>` 调 `decode_uddg`(uddg 后带 `&rut=` 尾);对**真抓样例** DDG HTML 调 `parse_ddg_results`
- **THEN** 前者得 `https://a.com/x`(正确止于 `&`、不吞 `rut` 尾;非重定向 / 广告 href → `None`);后者得 `{title,url,snippet}` 列表(url 为解码后真链、非 DDG 重定向)、至多 `MAX_SEARCH_RESULTS` 条
