# builtin-tools Specification

## Purpose
定义 12 个内置工具的行为契约:4 个本地只读工具(`list_dir` / `read_file` / `glob` / `grep`,权限级别 `ReadOnly` 且并发分类 `ParallelSafe`)、2 个联网工具(`web_fetch` / `web_search`,`Network`)、3 个变更类工具(`write_file` / `edit_file`,`Edit`;`run_shell`,`Execute`)与 3 个交互工具(`submit_plan` / `update_plan` / `ask_user`,`ReadOnly`、经注入 seam 呈递审批 / 上报进度 / 提问)，后八者均保持 `Exclusive`。四个本地读取工具把同步文件遍历迁入共享进程级 `Semaphore(4)` 约束的 `spawn_blocking` worker。Network 工具以 tool-owned preview 描述 canonical target 与真实 redirect / SSRF scope，逐次授权且拒绝时零 fetch；`web_fetch` 对初始 URL 与每个 redirect 逐跳施加 SSRF 护栏。关键立场是失败一律编码为 `ToolOutcome{is_error}` 回给模型而非 panic,输出受 `max_output_bytes` / `truncated` 约束,变更工具经权限门 `Deny` 时零副作用,`edit_file` 要求 `old_string` 唯一匹配。
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

### Requirement: submit_plan 提交结构化计划(Plan 模式)

`submit_plan` SHALL 接受结构化 plan `{ title, steps: [{ description, validation }] }`(`validation` = 该步可验收判据);`plan_only()==true`(仅 Plan 模式下发,见 tool-system);**`permission_level()==ReadOnly`**——呈递审批本质是只读动作(真正改动在批准后另起工具);**若定为 `Edit`/`Execute`,Plan 期一调用即被 agent-loop 的「非只读纵深拒」挡掉、approver 永不执行、plan 永远批不了(自我否决)**。经**可注入的 `PlanApprover` seam**(`: Send + Sync`,async)呈递审批,得 `PlanDecision {Approve | Reject(reason)}`:
- **Approve** → `ToolOutcome{content, is_error:false}`,`content` 以「计划已批准」开头并 SHALL 指示 agent 执行期用 `update_plan` 上报进度(每开始一步标记 `in_progress`、每完成一步标记 `done` 并附 validation 自检结果);权限模式 SHALL 由 approver 实现从 `Plan` 翻至 `AcceptEdits`(翻转在 oneshot 返回**之后**做、勿把 mode mutex 跨 `.await` 持;下一轮全工具可用、按 history 里的 plan 执行)。
- **Reject(reason)** → `ToolOutcome{content 含 reason, is_error:true}`(留 Plan、模型据理由修订)。

args 解析失败(缺 `title` / `steps`)SHALL → is_error、不 panic。审批经 mock approver 可测,不依赖 TUI。

#### Scenario: 批准返回成功

- **WHEN** 注入返 `Approve` 的 mock approver,execute 一个合法 plan
- **THEN** `is_error=false`,content 以「计划已批准」开头且含 `update_plan` 进度上报指示(mode 翻转由 approver 实现,契约见 tui-shell)

#### Scenario: 驳回带理由回模型

- **WHEN** 注入返 `Reject("先补测试")` 的 mock approver
- **THEN** `is_error=true`,content 含该理由

#### Scenario: 非法 plan 编码 is_error

- **WHEN** execute 一个缺 `steps` 的 args
- **THEN** `is_error=true`,不 panic

### Requirement: ask_user 向用户提结构化问题

`ask_user` SHALL 接受 `{ question, options: [{label, description}], allow_multi?, allow_other? }`;`permission_level=ReadOnly`、`plan_only()==false`(**任何模式可用**,Plan 期供研究澄清);经**可注入的 `UserPrompter` seam**(`: Send + Sync`,async)弹 A/B/C + 补充框、阻塞取 `Answer {selected, supplement}`,格式化(所选 label + 补充)回模型。args 解析失败(缺 `question`)SHALL → is_error、不 panic。经 mock prompter 可测,不依赖 TUI。

#### Scenario: 返回所选项 + 补充

- **WHEN** 注入返 `Answer{selected:["A"], supplement:Some("再考虑 X")}` 的 mock prompter,execute 一个带选项的问题
- **THEN** `is_error=false`,content 含所选 label 与补充文本

#### Scenario: 非法 args 编码 is_error

- **WHEN** execute 一个缺 `question` 的 args
- **THEN** `is_error=true`,不 panic

### Requirement: update_plan 上报计划进度(执行期)

`update_plan` SHALL 接受 `{ step: <1-based 整数>, status: "in_progress"|"done", validation_result?: string }`;`permission_level()==ReadOnly`、**`plan_only()==false`**(执行期用、任何模式可见,不占 schema-omit 的 plan_only 名额)。经**可注入的 `PlanProgressReporter` seam**(`: Send + Sync`)以 **fire-and-forget** 方式(同步 `report(update)`、**不要回值**——区别于 `submit_plan`/`ask_user` 的 oneshot 往返)上报一次 `PlanProgressUpdate {step, status, validation_result}`,随即返回 `ToolOutcome{content:"进度已记录", is_error:false}`。`status` **仅收 `in_progress` / `done`**——`Pending` 是面板激活初始态、不由 agent 上报,故 **`"pending"` 及任何其他值 SHALL 判为非法 → is_error**;实现 **MUST NOT** 直接给三态 `StepStatus` 派生 snake_case `Deserialize`(那样 `"pending"` 会静默反序列化成功、绕过校验),MUST 用独立 2 变体输入枚举(`in_progress`/`done`)或反序列化后显式 reject `Pending`。args 解析失败(缺 `step` / 非法 `status` / **`step==0`**)SHALL → is_error、不 panic(`step` 为 1-based,`0` 非法)。经 mock reporter 可测,不依赖 TUI。面板呈现与越界(含 `step==0` 下溢)忽略契约见 tui-shell。

#### Scenario: done 上报记录进度与验收

- **WHEN** 注入 mock reporter,execute `{step:2, status:"done", validation_result:"cargo test permission → 12 passed"}`
- **THEN** `is_error=false`,content 表进度已记录;reporter 收到一条 `PlanProgressUpdate{step:2, status:Done, validation_result:Some(...)}`

#### Scenario: in_progress 无验收亦合法

- **WHEN** execute `{step:1, status:"in_progress"}`(无 `validation_result`)
- **THEN** `is_error=false`;reporter 收到 `status:InProgress`、`validation_result:None`

#### Scenario: status 为 pending 判非法

- **WHEN** execute `{step:1, status:"pending"}`
- **THEN** `is_error=true`,不 panic(`pending` 不由 agent 上报;实现不得静默接受)

#### Scenario: 非法 args 编码 is_error

- **WHEN** execute 一个缺 `step`、`status` 非法(如 `"bogus"`)、或 `step:0` 的 args
- **THEN** `is_error=true`,不 panic

### Requirement: 内置工具并发分类与异步运行时隔离

12 个内置工具 SHALL 具有完整、固定的 `ToolConcurrency` 分类：`list_dir` / `read_file` / `glob` / `grep` MUST 为 `ParallelSafe`；`web_fetch` / `web_search` / `write_file` / `edit_file` / `run_shell` / `submit_plan` / `update_plan` / `ask_user` MUST 为 `Exclusive`。该分类不得由 `PermissionLevel` 推导；尤其三个交互 / 计划工具虽为 `ReadOnly`，仍必须独占。

四个 `ParallelSafe` 本地读取工具的同步文件读取、目录遍历与内容扫描工作 MUST NOT 在调用方 Tokio worker 上直接阻塞；它们 SHALL 把同步主体 offload 到 blocking worker，async `execute` 只等待结果。生产执行 SHALL 共用进程级 blocking limiter（上限 4）：调用取得 permit 后 MUST 把 permit 移入 blocking closure 并持有到真实同步工作结束，使调用 future / JoinHandle 被 drop 后旧 closure 与新 turn 的 blocking 工作合计仍不超过 4。blocking worker 无法 join（含 worker panic）时 MUST 返回 `ToolOutcome{is_error:true}`，不得 panic；既有 permission level、schema、gitignore、排序、分页、截断、错误文案与 `exit=None` 契约 MUST 保持不变。

#### Scenario: 四个本地读取工具显式 ParallelSafe

- **WHEN** 查询 `list_dir` / `read_file` / `glob` / `grep` 的 `concurrency()`
- **THEN** 四者均返回 `ParallelSafe`，且 `permission_level()` 仍为 `ReadOnly`

#### Scenario: 其余八个工具完整锁定 Exclusive

- **WHEN** 查询 `web_fetch` / `web_search` / `write_file` / `edit_file` / `run_shell` / `submit_plan` / `update_plan` / `ask_user` 的 `concurrency()`
- **THEN** 八者均返回 `Exclusive`；Network、Edit、Execute 与三个 `ReadOnly` 交互 / 计划工具都没有并行 opt-in

#### Scenario: 阻塞文件工作不占住调用方 worker

- **WHEN** 在 current-thread 测试 runtime 中让一个经同一 blocking helper 调度的受控文件工作发出 entered 后等待 std release，同时调度另一个独立 async probe，并由外部 OS watchdog 保证失败路径也会 release
- **THEN** async probe 的 ack 先于 release 被观察到，证明同步主体已离开调用方 Tokio worker；测试不得依赖同一 Tokio worker 上的 timeout 或 sleep 时长解死锁

#### Scenario: Interrupt 后新 turn 不突破进程级 blocking 上限

- **WHEN** 首批 4 个 blocking closure 已 entered 且其 awaiting futures 被取消，随即从新 turn 再提交 4 个读取工作
- **THEN** 旧 closure 结束并释放 permit 前新 closure 不得 entered，跨两批记录的 global max-active 始终 ≤4；测试使用独立 limiter 与 per-call ack，不污染并行测试

#### Scenario: offload 前后工具行为零回归

- **WHEN** 对四个本地读取工具复跑既有正常、失败、排序、分页、gitignore 与 UTF-8 截断测试
- **THEN** `ToolOutcome` 与 change 前逐字段一致；仅执行线程位置和并发分类改变

#### Scenario: blocking worker join failure 编码为工具错误

- **WHEN** 以测试 seam 令 blocking helper panic 或返回 JoinError
- **THEN** `execute` 返回 `is_error=true` 且包含稳定的 worker failure 说明，Agent 进程不 panic
