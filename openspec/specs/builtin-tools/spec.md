# builtin-tools Specification

## Purpose
定义 9 个内置工具的行为契约:6 个只读工具(`list_dir` / `read_file` / `glob` / `grep` / `web_fetch` / `web_search`,权限级别 `ReadOnly`)与 3 个变更类工具(`write_file` / `edit_file`,`Edit`;`run_shell`,`Execute`),覆盖各自的输入语义、输出截断(`max_output_bytes` / `truncated`)与 exit code 编码。关键立场是失败一律编码为 `ToolOutcome{is_error}` 回给模型而非 panic,变更类工具经权限门 `Deny` 时零副作用,`edit_file` 要求 `old_string` 唯一匹配、否则不写入。工具抽象与注册调度属 tool-system,权限判定机制属 permission-gate;本域仅约定各实体工具自身的行为。
## Requirements
### Requirement: list_dir 列目录(ReadOnly)

`list_dir` SHALL 列出指定目录(默认 `ToolContext.cwd`)下的条目,gitignore 感知(`ignore` crate);权限级别 `ReadOnly`;失败(路径不存在等)SHALL 编码为 `ToolOutcome{is_error: true}`,不 panic。

#### Scenario: 列出目录条目

- **WHEN** 对一个含若干文件的 tempdir 调用 `list_dir`
- **THEN** `ToolOutcome.content` 含这些条目,`is_error = false`

#### Scenario: 路径不存在

- **WHEN** 对一个不存在的路径调用 `list_dir`
- **THEN** 返回 `is_error = true` 的 `ToolOutcome`(不 panic)

### Requirement: read_file 读取与截断(ReadOnly)

`read_file` SHALL 读取文件内容,支持按**行**的 `offset` / `limit` 分页;当内容(分页后)超过 `ToolContext.max_output_bytes`(**字节**,按 UTF-8 字符边界截断)时 SHALL 截断并置 `ToolOutcome.truncated = true`;权限级别 `ReadOnly`;文件不存在 → is_error。

#### Scenario: 读取内容

- **WHEN** `read_file` 一个 tempdir 内已知内容的文件
- **THEN** `content` 等于该内容,`is_error = false`,`truncated = false`

#### Scenario: offset/limit 分页

- **WHEN** `read_file` 带 `offset` / `limit`
- **THEN** 只返回对应区间的内容

#### Scenario: 输出超限截断

- **WHEN** 文件内容超过 `max_output_bytes`
- **THEN** `content` 被截断,`truncated = true`

#### Scenario: 文件不存在

- **WHEN** `read_file` 一个不存在的路径
- **THEN** `is_error = true`

### Requirement: glob 文件匹配(ReadOnly)

`glob` SHALL 用 `globset` 按 pattern 匹配文件路径;权限级别 `ReadOnly`;无效 pattern → is_error。

#### Scenario: 匹配文件

- **WHEN** `glob` 一个匹配 tempdir 内若干文件的 pattern
- **THEN** `content` 列出匹配路径,`is_error = false`

#### Scenario: 无效 pattern

- **WHEN** `glob` 一个非法 pattern
- **THEN** `is_error = true`

### Requirement: grep 内容搜索与截断(ReadOnly)

`grep` SHALL 用 `ignore` 遍历 + `regex` 搜索内容,返回匹配行(含来源定位);输出超 `max_output_bytes` 时 SHALL 截断置 `truncated`;权限级别 `ReadOnly`;无效正则 → is_error。

#### Scenario: 找到匹配

- **WHEN** `grep` 一个在 tempdir 文件中存在的正则
- **THEN** `content` 含匹配行,`is_error = false`

#### Scenario: 无效正则

- **WHEN** `grep` 一个非法正则
- **THEN** `is_error = true`

#### Scenario: 输出超限截断

- **WHEN** 匹配输出超过 `max_output_bytes`
- **THEN** `truncated = true`

### Requirement: write_file 写入(Edit)

`write_file` SHALL 新建或覆盖写入文件内容;权限级别 `Edit`;写入失败 → is_error。

#### Scenario: 写入新文件

- **WHEN** `write_file` 到 tempdir 内一个新路径
- **THEN** 文件被创建且内容正确,`is_error = false`

#### Scenario: 覆盖既有文件

- **WHEN** `write_file` 到一个已存在的文件
- **THEN** 内容被覆盖

### Requirement: edit_file 唯一匹配替换(Edit)

`edit_file` SHALL 以 str-replace 编辑文件,要求 `old_string` 在文件中**恰好出现一次**;0 次或多于一次匹配 SHALL → is_error 且**不写入**;权限级别 `Edit`。

#### Scenario: 唯一匹配替换

- **WHEN** `edit_file` 的 `old_string` 在文件中恰好出现一次
- **THEN** 该处被替换为 `new_string`,文件更新,`is_error = false`

#### Scenario: 非唯一匹配报错且不改文件

- **WHEN** `old_string` 在文件中出现 0 次或多于一次
- **THEN** `is_error = true`,且文件未被修改

### Requirement: run_shell 执行(Execute)

`run_shell` SHALL 经平台 shell(Windows `cmd /C`、Unix `sh -c`)执行命令,捕获 stdout / stderr / exit code;SHALL 受 timeout 约束,超时则终止命令并 → is_error;输出超 `max_output_bytes` 时截断置 `truncated`;非零 exit → is_error;权限级别 `Execute`。

**console 独立性(Windows)**:子进程 SHALL 以 `CREATE_NO_WINDOW`(`0x0800_0000`,具名常量、`#[cfg(windows)]`)创建——不 attach 调用方 console,防止子进程重置 TUI 已设置的终端输入模式(`ENABLE_MOUSE_INPUT` 等,重置后终端把滚轮降级为方向键);stdout / stderr 经 pipe 捕获,MUST 不受该标志影响。非 Windows 平台无此问题,不加标志。

#### Scenario: 捕获输出与 exit

- **WHEN** `run_shell` 一个打印已知文本并成功退出的命令
- **THEN** `content` 含该输出与 exit code,`is_error = false`

#### Scenario: 超时终止

- **WHEN** `run_shell` 一个超过 timeout 仍未结束的命令
- **THEN** 命令被终止,`is_error = true`

#### Scenario: 非零退出

- **WHEN** 命令以非零 exit code 结束
- **THEN** `is_error = true`,`content` 含输出与 exit code

### Requirement: 变更工具经权限门拒绝时无副作用

经 Agent loop 调用的变更工具,在权限门返回 `Deny` 时 SHALL NOT 产生副作用(文件不被创建 / 修改、命令不被执行),且 is_error 的 `ToolResult` 入 history(由 `permission-gate` 保证;此处验证实体工具确受其约束)。

#### Scenario: 拒绝 write_file 无副作用

- **WHEN** Agent loop(注入 `DenyAll` decider)处理一个 `write_file` 的 tool_call
- **THEN** 目标文件未被创建,history 含一条 is_error 的 `ToolResult`

### Requirement: run_shell 退出码

`run_shell` SHALL 把进程退出码设入 `ToolOutcome.exit`(= 进程 `ExitStatus` 的 `code()`,被信号终止等无码情形为 `None`);其余 6 个内置工具(`list_dir` / `read_file` / `glob` / `grep` / `write_file` / `edit_file`)的 `ToolOutcome.exit` MUST 为 `None`。`run_shell` 既有 `content` / `is_error` / `truncated` 输出语义 MUST 不变。

#### Scenario: run_shell 设退出码、其余工具 None

- **WHEN** `run_shell` 执行一条退出码为 0 的命令
- **THEN** 其 `ToolOutcome.exit` 为 `Some(0)`,`content` / `is_error` 与既有一致

#### Scenario: 非进程类工具 exit 为 None

- **WHEN** 任一只读 / 写文件工具产出 `ToolOutcome`
- **THEN** 其 `exit` 为 `None`

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

