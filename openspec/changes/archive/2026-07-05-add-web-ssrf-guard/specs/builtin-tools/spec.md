# builtin-tools Delta

## MODIFIED Requirements

### Requirement: web_fetch 抓取网页文本(ReadOnly)

`web_fetch` SHALL 对给定 `url` 发 HTTP GET(带浏览器 User-Agent 与超时常量 `WEB_TIMEOUT`、响应体字节封顶防 OOM),将响应正文由 HTML 转为可读文本(剥 `<script>`/`<style>`、去标签、解 HTML 实体含 hex/十进制数字实体、折叠空白);权限级别 `ReadOnly`;文本超 `ToolContext.max_output_bytes`(字节、按 UTF-8 边界)时 SHALL 截断并置 `ToolOutcome.truncated = true`;非 2xx 响应、超时、网络错误、二进制 content-type(非 `text/*`;缺 `Content-Type` 视为可读)SHALL 编码为 `ToolOutcome{is_error: true}`,不 panic。

**SSRF 护栏**:`web_fetch` SHALL 对**初始 URL 及每一个重定向目标(逐跳)**施加同一道门:拒绝 scheme 非 `http`/`https`;host **从已解析的 URL 取**(数字编码 IP 于 parse 时已归一化;**不得从原始 URL 字符串手工切 host**,否则归一化丢失、编码绕过复活),IP 字面量直接判定、主机名经 DNS 解析后逐 IP 判定;凡落 loopback(`127.0.0.0/8`、`::1`)/ 私网(`10/8`、`172.16/12`、`192.168/16`)/ link-local(`169.254/16` 含云元数据 `169.254.169.254`、`fe80::/10`)/ unique-local(`fc00::/7`)/ CGNAT(`100.64/10`)/ NAT64(`64:ff9b::/96`)/ multicast / `0.0.0.0/8` / `240.0.0.0/4` 范围者 SHALL 拒绝(编码 `is_error`、**不发该请求**、不 panic);**DNS 解析失败或空结果 SHALL 亦拒绝(fail-closed)**。重定向 SHALL **不自动跟随**(`redirect::Policy::none`),由 `web_fetch` 手动逐跳过门、深度上限低值(如 3)。IP 范围判定 `is_blocked_ip`、URL 前置检查 `precheck_url`、已解析裁决 `check_resolved`(空集=拒)SHALL 抽为纯函数(headless 强制 TDD)。**已知残留**:对抗性 DNS rebinding(护栏解析与连接层独立再解析,攻击者以 fast-TTL 在两次解析间翻转记录)——注:`check_resolved` 检查 `to_socket_addrs` 返回的**全部**地址、任一内网即拒,故稳定的多记录混合域已被拦、非残留;残留仅限主动 rebinding。升级 = 自定义 `dns::Resolve` pin 解析(使「检查即连接」);另 v6 内嵌 v4 纵深(6to4/Teredo 等)本次未拦。T0 可接受。

HTTP 抓取经可注入的 `WebFetcher`(`: Send + Sync`)seam(async),测试以 mock fetcher 提供 canned 内容/错误,不依赖真实网络。HTML→文本 SHALL 抽为纯函数 `html_to_text`(headless 强制 TDD)。

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

#### Scenario: 拒绝内网/loopback/link-local 目标(SSRF 护栏)

- **WHEN** 对 `http://127.0.0.1:8765/`、`http://169.254.169.254/…`、数字编码 `http://2130706433/`、或 DNS 解析到私网 IP 的主机名调用 `web_fetch`
- **THEN** 返回 `is_error = true`(内容注明 blocked)、**不发出该请求**、不 panic

#### Scenario: 重定向到内网被拦(逐跳同门)

- **WHEN** `web_fetch` 一个公网 URL,其响应 `302` 到内网目标(`Location` 为内网 IP 字面量、或解析到内网的主机名)
- **THEN** 该跳目标过同一道门被拒 → `is_error = true`、**不跟随连接到内网**、不 panic

#### Scenario: is_blocked_ip 纯函数(可单测)

- **WHEN** 对 loopback / 私网 / link-local / unique-local / CGNAT / NAT64(`64:ff9b::/96`)/ multicast / `0.0.0.0/8` / `240/4` 各范围(v4 / v6 / v4-mapped)与公网 IP 调 `is_blocked_ip`
- **THEN** 前者返回 `true`、公网返回 `false`
